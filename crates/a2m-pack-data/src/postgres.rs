//! The server store: PostgreSQL, through `tokio-postgres`. Nothing outside
//! this module names that crate.
//!
//! A connection is held to its one database, its one schema, and its one
//! identity by the server itself, whatever a statement says, and each setting
//! is read back before the connection is used:
//!
//! - The link is to the loopback address only, without TLS, which on the
//!   loopback address protects nothing a process of the same machine could
//!   not read. Off it, the pack refuses before anything is sent: this build
//!   makes no encrypted link, and never an unencrypted one.
//! - The role and its password come from the session keeper's lease. The
//!   role must be no superuser, must not bypass row security, replicate, or
//!   make roles, and must own nothing in the schema: otherwise the server's own rights
//!   are no bar, and the pack stops.
//! - `search_path` is the surface's one schema, `standard_conforming_strings`
//!   is on (so that a backslash in a text is a backslash, as the form check
//!   reads it), the server's text is UTF-8, and a reader's transactions are
//!   read-only by default.
//! - Every transaction is begun with its access and isolation stated, its
//!   access is read back, and the session's role is read back as the leased
//!   one, so that a statement that changed the role or the access is found
//!   before the next one runs.
//! - Every value is bound as a parameter of the extended protocol, as text
//!   that the server's own input function reads: a decimal reaches a
//!   `numeric` column exactly, never through a float. Every value comes back
//!   in the binary form and is turned into exact text here: `numeric` digit
//!   by digit, a date as `YYYY-MM-DD`, a moment with its zone as UTC. A type
//!   with no rule is refused.
//!
//! Every method blocks: the driver calls it on a blocking thread, and it waits
//! there on the engine's own runtime, where the connection's task runs.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use a2m_auth::{ChallengeScheme, CredentialSink, SessionLease};
use a2m_core::{ScopedHost, Secret, SurfacePlan};
use a2m_provider::StepFailure;
use bytes::BytesMut;
use tokio::runtime::Handle;
use tokio_postgres::config::SslMode;
use tokio_postgres::error::SqlState;
use tokio_postgres::types::{Format, FromSql, IsNull, ToSql, Type, to_sql_checked};
use tokio_postgres::{Client, Config, NoTls, Row, Statement};

use crate::driver::PoolKey;
use crate::model::{Access, Isolation, SurfaceFields};
use crate::names::codes;
use crate::shape::Dialect;
use crate::store::{
    Bound, Catalog, Cell, ColumnInfo, CommitFailed, Conn, ForeignKey, Interrupter, Leads, Opener,
    Ran, Refusal, RunLimits, ServerAt, Shape, StoreAt, TableInfo, Unseen,
};

/// The schema statements are read in when a surface names none.
const DEFAULT_SCHEMA: &str = "public";
/// How long a connection may take to be made.
const CONNECT_LIMIT: Duration = Duration::from_secs(10);
/// How long a transaction may stay open with nothing sent, before the server
/// ends the session.
const IDLE_IN_TRANSACTION_MS: u32 = 60_000;
/// The server's predefined roles whose rights reach beyond the rows of a
/// schema: its files, its programs, other sessions, every table, and what
/// the server reports of itself. A role that is a member of any is refused,
/// as a superuser is: the store's own rights would then be no bar.
const REACHING_ROLES: &str = "'pg_read_server_files', 'pg_write_server_files', \
     'pg_execute_server_program', 'pg_signal_backend', 'pg_read_all_data', 'pg_write_all_data', \
     'pg_monitor', 'pg_read_all_settings', 'pg_read_all_stats'";

// ---- the surface and the lease ----

/// Where a `database_server` surface's store is, with the lease's identity,
/// and the key its connections are kept under.
pub(crate) fn server_of(
    surface: &SurfacePlan,
    fields: &SurfaceFields,
    lease: Option<&SessionLease>,
) -> Result<(StoreAt, PoolKey), StepFailure> {
    // Judged by the one question every check of a plain link asks
    // (decision 0240): the address as written, never a name.
    let host: Option<IpAddr> = surface.address.parse().ok();
    let Some(host) = host.filter(|ip| !ScopedHost::of_ip(*ip).may_leave_this_machine()) else {
        return Err(StepFailure::not_sent(codes::server::row::LINK_NOT_ENCRYPTED.failure(
            format!(
                "The surface `{}` is not on the loopback address, and this version of the data pack makes no encrypted link to a server store. An unencrypted link is never made, and nothing was sent.",
                surface.name
            ),
            "Reach the store at `127.0.0.1` or `::1`, written as an address and not a name; an encrypted link to another machine is not built yet.",
        )));
    };
    let (Some(port), Some(database)) = (fields.port, fields.database.clone()) else {
        return Err(StepFailure::not_sent(
            codes::row::UNEXPECTED_STATUS.failure(
                "The surface states no port or no database.",
                "Compile the target again: the surface kind requires both.",
            ),
        ));
    };
    let schema = fields
        .schema
        .clone()
        .unwrap_or_else(|| DEFAULT_SCHEMA.to_owned());
    let Some(lease) = lease else {
        return Err(not_carried(
            "The surface has no session, so there is no identity to sign in as.",
        ));
    };
    let mut sink = PasswordSink::default();
    lease.apply(&mut sink).map_err(StepFailure::not_sent)?;
    let (Some((user, password)), false) = (sink.pair.take(), sink.other) else {
        return Err(not_carried(
            "The session's credential is not one user and one password, which is all a server store is signed in with.",
        ));
    };
    let at = ServerAt {
        host,
        port,
        database,
        schema,
        user,
        password: Arc::new(password),
    };
    // Kept under everything that says where a connection leads and as whom,
    // so that a surface published again with another port, database, or
    // schema never takes a connection of the old one.
    let key: PoolKey = (
        surface.name.as_str().to_owned(),
        lease.session().to_string(),
        lease.epoch(),
        at.leads(),
    );
    Ok((StoreAt::Server(at), key))
}

fn not_carried(message: &str) -> StepFailure {
    StepFailure::not_sent(codes::server::row::CREDENTIAL_NOT_CARRIED.failure(
        message,
        "Sign the target in with the provider `http_basic`, whose setting `user` is the store's role and whose credential `password` is its password.",
    ))
}

/// Where the lease's credential is put. A server store is signed in with one
/// user and one password. The provider that holds exactly that, `http_basic`,
/// places them as the `Authorization` value of the basic scheme, which is
/// read back into the two here; anything else a provider places is noted,
/// and the run stops.
#[derive(Default)]
struct PasswordSink {
    pair: Option<(String, Secret)>,
    other: bool,
}

impl CredentialSink for PasswordSink {
    fn header(&mut self, name: &str, value: &Secret) {
        let pair = name
            .eq_ignore_ascii_case("authorization")
            .then(|| value.expose().strip_prefix("Basic "))
            .flatten()
            .and_then(decode_base64)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| {
                let (user, password) = text.split_once(':')?;
                Some((user.to_owned(), Secret::new(password)))
            });
        match pair {
            Some(pair) if self.pair.is_none() => self.pair = Some(pair),
            _ => self.other = true,
        }
    }

    fn cookie(&mut self, _origin: &str, _name: &str, _value: &Secret) {
        self.other = true;
    }

    fn tls_identity(&mut self, _certificate: &[u8], _key: &Secret) {
        self.other = true;
    }

    fn challenge_credential(&mut self, _scheme: ChallengeScheme, _user: &str, _secret: &Secret) {
        self.other = true;
    }
}

/// The standard base64 alphabet, with padding. `None` for anything else.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = text.trim_end_matches('=').as_bytes();
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut word = 0_u32;
        for (place, byte) in chunk.iter().enumerate() {
            word |= value(*byte)? << (18 - 6 * place);
        }
        let [_, first, second, third] = word.to_be_bytes();
        out.push(first);
        if chunk.len() > 2 {
            out.push(second);
        }
        if chunk.len() > 3 {
            out.push(third);
        }
    }
    Some(out)
}

// ---- the store ----

/// The server store.
pub(crate) struct Postgres;

impl Opener for Postgres {
    fn store(&self) -> &'static str {
        "postgres"
    }

    fn quote(&self, identifier: &str) -> Option<String> {
        // Double quotes, a quote inside doubled. A quoted name is kept as
        // written: the server folds only names that are not quoted.
        if identifier.is_empty() || identifier.chars().any(|c| c.is_control() || c == '\0') {
            return None;
        }
        Some(format!("\"{}\"", identifier.replace('"', "\"\"")))
    }

    fn placeholder(&self, position: usize) -> String {
        format!("${position}")
    }

    fn dialect(&self) -> Dialect {
        Dialect::Server
    }

    fn returns_assigned_key(&self) -> bool {
        true
    }

    fn pooled(&self) -> bool {
        true
    }

    fn open(&self, at: &StoreAt, access: Access) -> Result<Box<dyn Conn>, Refusal> {
        let StoreAt::Server(server) = at else {
            return Err(Refusal::NotGuarded);
        };
        // A name the server could read as more than a name is refused. The
        // surface's schema is a plain name; this holds it there.
        if !server
            .schema
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            || !server.host.is_loopback()
        {
            return Err(Refusal::NotGuarded);
        }
        let runtime = Handle::try_current().map_err(|_| Refusal::Unavailable)?;
        let mut config = Config::new();
        config
            .host(server.host.to_string())
            .hostaddr(server.host)
            .port(server.port)
            .dbname(&server.database)
            .user(&server.user)
            .password(server.password.expose())
            .ssl_mode(SslMode::Disable)
            .connect_timeout(CONNECT_LIMIT)
            .application_name("anything2mcp")
            .options(format!(
                "-c search_path={} -c default_transaction_read_only={} -c idle_in_transaction_session_timeout={IDLE_IN_TRANSACTION_MS} -c TimeZone=UTC",
                server.schema,
                match access {
                    Access::ReadOnly => "on",
                    Access::ReadWrite => "off",
                }
            ));
        let (client, connection) = runtime
            .block_on(config.connect(NoTls))
            .map_err(|error| at_open(&error))?;
        // The connection's task runs on the engine's runtime, so that the
        // link is read while this thread waits, and an end of the link is
        // seen at once.
        runtime.spawn(async move {
            let _ = connection.await;
        });
        let mut conn = PgConn {
            client,
            runtime,
            access,
            user: server.user.clone(),
            schema: server.schema.clone(),
            leads: server.leads(),
            reported: None,
            open: false,
            prepared: BTreeMap::new(),
        };
        conn.confine()?;
        Ok(Box::new(conn))
    }
}

struct PgConn {
    client: Client,
    runtime: Handle,
    access: Access,
    /// The role the lease signed in as.
    user: String,
    schema: String,
    /// Where the connection was opened to, and as whom.
    leads: Leads,
    /// The server's own address and port, as the server reported them when
    /// the connection was opened (`None` for each on a local socket behind a
    /// relay): asked again before every transaction, and held to these.
    reported: Option<(Option<String>, Option<i64>)>,
    /// Whether a transaction is open, as far as this connection knows.
    open: bool,
    /// The statements prepared on this connection, by their text.
    prepared: BTreeMap<String, Statement>,
}

/// A value bound to a parameter, sent as text that the server's own input
/// function for the parameter's type reads: an exact decimal reaches a
/// `numeric` column as its digits.
#[derive(Debug)]
struct AsText<'a>(&'a Bound);

impl ToSql for AsText<'_> {
    fn to_sql(
        &self,
        _ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self.0 {
            Bound::Null => return Ok(IsNull::Yes),
            Bound::Whole(number) => out.extend_from_slice(number.to_string().as_bytes()),
            Bound::Text(text) => out.extend_from_slice(text.as_bytes()),
        }
        Ok(IsNull::No)
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    fn encode_format(&self, _ty: &Type) -> Format {
        Format::Text
    }

    to_sql_checked!();
}

/// One stored value, read from the binary form of its type.
struct Read(Cell);

impl<'a> FromSql<'a> for Read {
    fn from_sql(
        ty: &Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(cell(ty, raw)))
    }

    fn from_sql_null(_ty: &Type) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(Cell::Null))
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }
}

fn whole<const N: usize>(raw: &[u8]) -> Option<[u8; N]> {
    raw.try_into().ok()
}

/// A value of the binary form of its type, as the pack's own cell.
fn cell(ty: &Type, raw: &[u8]) -> Cell {
    let is = |known: &Type| ty == known;
    let found = if is(&Type::INT2) {
        whole::<2>(raw).map(|b| Cell::Whole(i64::from(i16::from_be_bytes(b))))
    } else if is(&Type::INT4) {
        whole::<4>(raw).map(|b| Cell::Whole(i64::from(i32::from_be_bytes(b))))
    } else if is(&Type::INT8) {
        whole::<8>(raw).map(|b| Cell::Whole(i64::from_be_bytes(b)))
    } else if is(&Type::OID) {
        whole::<4>(raw).map(|b| Cell::Whole(i64::from(u32::from_be_bytes(b))))
    } else if is(&Type::BOOL) {
        match raw {
            [0] => Some(Cell::Whole(0)),
            [1] => Some(Cell::Whole(1)),
            _ => None,
        }
    } else if [
        Type::TEXT,
        Type::VARCHAR,
        Type::BPCHAR,
        Type::NAME,
        Type::CHAR,
    ]
    .iter()
    .any(is)
    {
        std::str::from_utf8(raw)
            .ok()
            .map(|text| Cell::Text(text.to_owned()))
    } else if is(&Type::NUMERIC) {
        numeric_text(raw).map(Cell::Text)
    } else if is(&Type::DATE) {
        whole::<4>(raw)
            .map(i32::from_be_bytes)
            .and_then(date_text)
            .map(Cell::Text)
    } else if is(&Type::TIMESTAMPTZ) {
        whole::<8>(raw)
            .map(i64::from_be_bytes)
            .and_then(moment_text)
            .map(Cell::Text)
    } else if is(&Type::FLOAT4) || is(&Type::FLOAT8) {
        Some(Cell::Float)
    } else if is(&Type::BYTEA) {
        Some(Cell::Bytes)
    } else {
        // A moment with no zone, a time, an interval, a list, a document,
        // and every other type: no rule reads them.
        Some(Cell::Unsupported)
    };
    found.unwrap_or(Cell::Unsupported)
}

/// A `numeric` in its binary form as exact decimal text, with as many digits
/// after the point as its scale states. `None` for not-a-number, an infinity,
/// or a form that does not read.
fn numeric_text(raw: &[u8]) -> Option<String> {
    let word =
        |at: usize| -> Option<u16> { Some(u16::from_be_bytes([*raw.get(at)?, *raw.get(at + 1)?])) };
    let count = usize::from(word(0)?);
    let weight = i32::from(i16::from_be_bytes(word(2)?.to_be_bytes()));
    let sign = word(4)?;
    let scale = usize::from(word(6)?);
    if raw.len() != 8 + 2 * count {
        return None;
    }
    let negative = match sign {
        0x0000 => false,
        0x4000 => true,
        // Not a number, or an infinity: no exact decimal.
        _ => return None,
    };
    let mut digits = Vec::with_capacity(count);
    for at in 0..count {
        let digit = word(8 + 2 * at)?;
        if digit > 9999 {
            return None;
        }
        digits.push(digit);
    }
    // Each base-10000 digit at its place: place `weight` is the first
    // group before the point.
    let group = |place: i32| -> u16 {
        usize::try_from(weight - place)
            .ok()
            .and_then(|at| digits.get(at))
            .copied()
            .unwrap_or(0)
    };
    let mut whole = String::new();
    for place in (0..=weight.max(0)).rev() {
        let value = group(place);
        if whole.is_empty() {
            if value != 0 || place == 0 {
                whole.push_str(&value.to_string());
            }
        } else {
            whole.push_str(&format!("{value:04}"));
        }
    }
    if whole.is_empty() {
        whole.push('0');
    }
    let mut fraction = String::new();
    let mut place = -1;
    while fraction.len() < scale {
        fraction.push_str(&format!("{:04}", group(place)));
        place -= 1;
    }
    fraction.truncate(scale);
    let mut text = String::new();
    if negative && (whole != "0" || fraction.bytes().any(|byte| byte != b'0')) {
        text.push('-');
    }
    text.push_str(&whole);
    if scale > 0 {
        text.push('.');
        text.push_str(&fraction);
    }
    Some(text)
}

/// Days since 2000-01-01 as `YYYY-MM-DD`, for years 1 to 9999.
fn date_text(days: i32) -> Option<String> {
    let (year, month, day) = civil(i64::from(days) + 10_957)?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// Microseconds since 2000-01-01 00:00 UTC as an RFC 3339 moment in UTC, with
/// as many digits of the second as it has.
fn moment_text(micros: i64) -> Option<String> {
    let seconds = micros.div_euclid(1_000_000);
    let fraction = micros.rem_euclid(1_000_000);
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil(days + 10_957)?;
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    );
    if fraction > 0 {
        let digits = format!("{fraction:06}");
        text.push('.');
        text.push_str(digits.trim_end_matches('0'));
    }
    text.push('Z');
    Some(text)
}

/// Days since 1970-01-01 as a year, a month, and a day of the proleptic
/// Gregorian calendar; `None` outside years 1 to 9999.
fn civil(days: i64) -> Option<(i64, i64, i64)> {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let of_era = z.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (1..=9999).contains(&year).then_some((year, month, day))
}

// ---- what an error means ----

fn constraint(state: &SqlState) -> &'static str {
    match state.code() {
        "23505" => "unique",
        "23514" => "check",
        "23502" => "not-null",
        "23503" => "foreign-key",
        "23P01" => "exclusion",
        _ => "other",
    }
}

/// The name of the constraint, and its table and column, as the server's
/// error carries them in fields of their own. Those fields name schema; the
/// message and its detail, which quote the row, are never read.
fn named_by(error: &tokio_postgres::Error) -> Option<String> {
    let found = error.as_db_error()?;
    let mut said = Vec::new();
    if let Some(name) = found.constraint() {
        said.push(format!("constraint {name}"));
    }
    if let Some(table) = found.table() {
        said.push(format!("table {table}"));
    }
    if let Some(column) = found.column() {
        said.push(format!("column {column}"));
    }
    (!said.is_empty()).then(|| said.join(", "))
}

/// What an error means while a statement runs. The server's own message is
/// never kept: it may quote a row.
fn refusal(error: &tokio_postgres::Error) -> Refusal {
    let Some(state) = error.code() else {
        // No answer of the server: the link ended, or the client refused.
        return if error.is_closed() {
            Refusal::Unavailable
        } else {
            Refusal::Other(0)
        };
    };
    let code = state.code();
    match code {
        "57014" => Refusal::TimedOut,
        "55P03" | "40P01" | "40001" | "53300" => Refusal::Locked,
        "25006" => Refusal::ReadOnly,
        "42501" => Refusal::Denied,
        _ if code.starts_with("23") => Refusal::Constraint {
            kind: constraint(state),
            named: named_by(error),
        },
        // A value the column's type does not read.
        _ if code.starts_with("22") => Refusal::Constraint {
            kind: "value",
            named: None,
        },
        // A table, a column, a function, or the statement's syntax.
        _ if code.starts_with("42") => Refusal::Shape,
        _ if code.starts_with("08") || code.starts_with("57P") => Refusal::Unavailable,
        "54000" | "54001" => Refusal::TooLarge,
        _ => Refusal::Other(0),
    }
}

/// Whether a server's error ends its session rather than the statement:
/// the link (`08`), an operator or a crash (`57P`), the system (`58`), or the
/// server's own failure (`XX`).
fn session_ends(state: &SqlState) -> bool {
    let code = state.code();
    ["08", "57P", "58", "XX"]
        .iter()
        .any(|class| code.starts_with(class))
}

/// What an error means while a connection is made.
fn at_open(error: &tokio_postgres::Error) -> Refusal {
    match error.code().map(SqlState::code) {
        // The password or the role was refused.
        Some(code) if code.starts_with("28") => Refusal::Denied,
        Some("42501") => Refusal::Denied,
        _ => Refusal::Unavailable,
    }
}

fn texts(row: &Row, at: usize) -> Option<String> {
    match row.try_get::<_, Read>(at).ok()?.0 {
        Cell::Text(text) => Some(text),
        _ => None,
    }
}

fn number(row: &Row, at: usize) -> Option<i64> {
    match row.try_get::<_, Read>(at).ok()?.0 {
        Cell::Whole(number) => Some(number),
        _ => None,
    }
}

/// A key's rule, as the server's catalog codes it, in the words of the
/// embedded store's catalog.
fn key_rule(code: &str) -> &'static str {
    match code {
        "c" => "CASCADE",
        "n" => "SET NULL",
        "d" => "SET DEFAULT",
        "r" => "RESTRICT",
        _ => "NO ACTION",
    }
}

impl PgConn {
    fn wait<T>(&self, work: impl std::future::Future<Output = T>) -> T {
        self.runtime.block_on(work)
    }

    /// The rows of one of the pack's own queries, each value as text or a
    /// whole number.
    fn own(&self, sql: &str, params: &[Bound]) -> Result<Vec<Row>, Refusal> {
        let bound: Vec<AsText<'_>> = params.iter().map(AsText).collect();
        self.wait(self.client.query_raw(sql, bound.iter()))
            .map_err(|error| refusal(&error))
            .and_then(|stream| self.collect(stream, usize::MAX).map(|(rows, _)| rows))
    }

    /// Up to `most` rows of a stream, and whether there were more.
    fn collect(
        &self,
        stream: tokio_postgres::RowStream,
        most: usize,
    ) -> Result<(Vec<Row>, bool), Refusal> {
        let mut stream = Box::pin(stream);
        let mut rows = Vec::new();
        loop {
            let next = self.wait(std::future::poll_fn(|cx| {
                futures_core::Stream::poll_next(stream.as_mut(), cx)
            }));
            match next {
                None => return Ok((rows, false)),
                Some(Err(error)) => return Err(refusal(&error)),
                Some(Ok(_)) if rows.len() >= most => return Ok((rows, true)),
                Some(Ok(row)) => rows.push(row),
            }
        }
    }

    /// Reads back what the connection is held to, before it is used.
    fn confine(&mut self) -> Result<(), Refusal> {
        let rows = self.own(
            &format!(
                "SELECT current_setting('search_path'), current_setting('default_transaction_read_only'), \
                 current_setting('standard_conforming_strings'), current_setting('server_encoding'), \
                 session_user::text, current_user::text, \
                 (r.rolsuper OR r.rolbypassrls OR r.rolreplication OR r.rolcreaterole), \
                 (SELECT count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                  WHERE n.nspname = $1 AND pg_catalog.pg_has_role(c.relowner, 'MEMBER')), \
                 (SELECT count(*) FROM pg_catalog.pg_roles p WHERE p.rolname IN ({REACHING_ROLES}) \
                  AND pg_catalog.pg_has_role(current_user, p.oid, 'MEMBER')), \
                 current_database()::text, pg_catalog.inet_server_addr()::text, pg_catalog.inet_server_port() \
                 FROM pg_catalog.pg_roles r WHERE r.rolname = current_user"
            ),
            &[Bound::Text(self.schema.clone())],
        )?;
        let row = rows.first().ok_or(Refusal::NotGuarded)?;
        let read_only = match self.access {
            Access::ReadOnly => "on",
            Access::ReadWrite => "off",
        };
        let held = texts(row, 0).as_deref() == Some(self.schema.as_str())
            && texts(row, 1).as_deref() == Some(read_only)
            && texts(row, 2).as_deref() == Some("on")
            && texts(row, 3).as_deref() == Some("UTF8")
            && texts(row, 4).as_deref() == Some(self.user.as_str())
            && texts(row, 5).as_deref() == Some(self.user.as_str());
        if !held {
            return Err(Refusal::NotGuarded);
        }
        if texts(row, 9).as_deref() != Some(self.leads.database.as_str()) {
            return Err(Refusal::Elsewhere);
        }
        if number(row, 6) != Some(0) || number(row, 7) != Some(0) || number(row, 8) != Some(0) {
            return Err(Refusal::TooPowerful);
        }
        self.reported = Some((texts(row, 10), number(row, 11)));
        Ok(())
    }

    fn statement(&mut self, text: &str) -> Result<Statement, Refusal> {
        if let Some(found) = self.prepared.get(text) {
            return Ok(found.clone());
        }
        let prepared =
            self.wait(self.client.prepare(text))
                .map_err(|error| match refusal(&error) {
                    Refusal::Other(_) => Refusal::Shape,
                    other => other,
                })?;
        self.prepared.insert(text.to_owned(), prepared.clone());
        Ok(prepared)
    }
}

/// A duration as whole milliseconds of at least one, for a setting.
fn millis(wait: Duration) -> String {
    wait.as_millis().clamp(1, u128::from(u32::MAX)).to_string()
}

impl Conn for PgConn {
    fn access(&self) -> Access {
        self.access
    }

    fn begin(&mut self, isolation: Isolation, wait: Duration) -> Result<(), Refusal> {
        let level = match isolation {
            Isolation::ReadCommitted => "READ COMMITTED",
            Isolation::RepeatableRead => "REPEATABLE READ",
            Isolation::Serializable => "SERIALIZABLE",
        };
        let access = match self.access {
            Access::ReadOnly => "READ ONLY",
            Access::ReadWrite => "READ WRITE",
        };
        self.wait(
            self.client
                .batch_execute(&format!("BEGIN ISOLATION LEVEL {level} {access}")),
        )
        .map_err(|error| refusal(&error))?;
        self.open = true;
        // The access and the role, read back inside the transaction, and
        // where the server says the connection is: its database, its
        // `search_path`, the reading of a backslash, and its own address
        // and port. A statement of an earlier transaction that changed any
        // of them, or a connection that does not lead where it was opened
        // to, is found here, before anything else runs: before every write,
        // and before every read-back.
        let rows = self.own(
            "SELECT set_config('lock_timeout', $1, true), current_setting('transaction_read_only'), \
             session_user::text, current_user::text, current_database()::text, \
             current_setting('search_path'), current_setting('standard_conforming_strings'), \
             pg_catalog.inet_server_addr()::text, pg_catalog.inet_server_port()",
            &[Bound::Text(millis(wait))],
        );
        let row = rows.as_ref().ok().and_then(|rows| rows.first());
        let held = row.is_some_and(|row| {
            texts(row, 1).as_deref()
                == Some(match self.access {
                    Access::ReadOnly => "on",
                    Access::ReadWrite => "off",
                })
                && texts(row, 6).as_deref() == Some("on")
        });
        let here = row.is_some_and(|row| {
            texts(row, 2).as_deref() == Some(self.leads.user.as_str())
                && texts(row, 3).as_deref() == Some(self.leads.user.as_str())
                && texts(row, 4).as_deref() == Some(self.leads.database.as_str())
                && texts(row, 5).as_deref() == Some(self.leads.schema.as_str())
                && self.reported.as_ref() == Some(&(texts(row, 7), number(row, 8)))
        });
        if !held || !here {
            self.rollback();
            return Err(match rows {
                Err(refusal) => refusal,
                Ok(_) if !here => Refusal::Elsewhere,
                Ok(_) => Refusal::NotGuarded,
            });
        }
        Ok(())
    }

    fn in_transaction(&self) -> bool {
        self.open
    }

    fn describe(&mut self, text: &str) -> Result<Shape, Refusal> {
        let prepared = self.statement(text)?;
        Ok(Shape {
            params: prepared.params().len(),
            columns: prepared
                .columns()
                .iter()
                .map(|column| column.name().to_owned())
                .collect(),
            // The server cannot say before it runs a statement; the read-only
            // transaction refuses one that writes.
            reads_only: None,
        })
    }

    fn run(&mut self, text: &str, params: &[Bound], limits: &RunLimits) -> Result<Ran, Refusal> {
        let prepared = self.statement(text)?;
        if prepared.params().len() != params.len() {
            return Err(Refusal::Shape);
        }
        if self.access == Access::ReadOnly && prepared.columns().is_empty() {
            return Err(Refusal::NotAllowed);
        }
        // The server's own limit on the statement, and on a lock wait, for
        // this transaction only. A lock wait ends first, so that a lock held
        // by another connection is named as that and not as a slow statement.
        self.own(
            "SELECT set_config('statement_timeout', $1, true), set_config('lock_timeout', $2, true)",
            &[
                Bound::Text(millis(limits.wait)),
                Bound::Text(millis(limits.wait.mul_f64(0.9))),
            ],
        )?;
        let bound: Vec<AsText<'_>> = params.iter().map(AsText).collect();
        let stream = self
            .wait(self.client.query_raw(&prepared, bound.iter()))
            .map_err(|error| refusal(&error))?;
        let mut stream = Box::pin(stream);
        let columns: Vec<String> = prepared
            .columns()
            .iter()
            .map(|column| column.name().to_owned())
            .collect();
        let mut ran = Ran {
            columns,
            ..Ran::default()
        };
        loop {
            let next = self.wait(std::future::poll_fn(|cx| {
                futures_core::Stream::poll_next(stream.as_mut(), cx)
            }));
            let row = match next {
                None => break,
                Some(Err(error)) => return Err(refusal(&error)),
                Some(Ok(row)) => row,
            };
            if ran.rows.len() >= limits.rows as usize {
                ran.more = true;
                break;
            }
            let mut cells = Vec::with_capacity(ran.columns.len());
            for at in 0..ran.columns.len() {
                let found = row
                    .try_get::<_, Read>(at)
                    .map_or(Cell::Unsupported, |read| read.0);
                ran.bytes += match &found {
                    Cell::Text(text) => text.len() as u64,
                    Cell::Whole(_) => 8,
                    _ => 0,
                };
                cells.push(found);
            }
            if ran.bytes > limits.bytes {
                return Err(Refusal::TooLarge);
            }
            ran.rows.push(cells);
        }
        if self.access == Access::ReadWrite && !ran.more {
            // The server's own count of the rows the statement changed.
            ran.changed = stream.rows_affected().unwrap_or(0);
        }
        Ok(ran)
    }

    fn commit(&mut self, wait: Duration) -> Result<(), CommitFailed> {
        if !self.open {
            return Err(CommitFailed::NotCommitted(Refusal::Other(0)));
        }
        // A link that has already ended sent nothing: the server rolled the
        // transaction back when the session ended.
        if self.client.is_closed() {
            self.open = false;
            return Err(CommitFailed::NotCommitted(Refusal::Unavailable));
        }
        // Held to the step's time: a server that holds its answer (a
        // synchronous standby that is gone waits without end) leaves the
        // write in doubt at the deadline, and the connection is closed.
        // Nothing is cancelled: a commit cut short is the doubt itself.
        let done = self.wait(tokio::time::timeout(
            wait,
            self.client.batch_execute("COMMIT"),
        ));
        self.open = false;
        let Ok(done) = done else {
            self.open = true;
            return Err(CommitFailed::Unknown);
        };
        match done {
            Ok(()) => Ok(()),
            // The server answered with an error of the transaction itself (a
            // conflict, a constraint checked at the end): it did not commit,
            // and the transaction is over.
            Err(error) if error.code().is_some_and(|state| !session_ends(state)) => {
                Err(CommitFailed::NotCommitted(refusal(&error)))
            }
            // The link ended after the `COMMIT` was sent, or the server ended
            // the session in answer to it (it was stopped, its link failed, or
            // it failed inside): nothing says whether it committed. The client
            // reports the first as an error with no server code; it reports
            // a session the server ended as that session's own error, such
            // as `57P01`.
            Err(_) => {
                self.open = true;
                Err(CommitFailed::Unknown)
            }
        }
    }

    fn rollback(&mut self) {
        if self.open && self.wait(self.client.batch_execute("ROLLBACK")).is_ok() {
            self.open = false;
        }
    }

    fn catalog(&mut self, tables: usize) -> Result<Catalog, Refusal> {
        let limit = i64::try_from(tables.saturating_add(1)).unwrap_or(i64::MAX);
        let names = self.own(
            "SELECT c.oid::int8, c.relname::text, (c.relkind IN ('v', 'm')) FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'v', 'm', 'f') ORDER BY c.relname LIMIT $2",
            &[Bound::Text(self.schema.clone()), Bound::Whole(limit)],
        )?;
        let truncated = names.len() > tables;
        let mut catalog = Catalog {
            tables: Vec::new(),
            truncated,
        };
        for row in names.iter().take(tables) {
            let (Some(oid), Some(name), Some(view)) =
                (number(row, 0), texts(row, 1), number(row, 2))
            else {
                return Err(Refusal::Other(0));
            };
            let oid = Bound::Whole(oid);
            let columns = self
                .own(
                    "SELECT a.attname::text, pg_catalog.format_type(a.atttypid, a.atttypmod), a.attnotnull, \
                     coalesce((SELECT k.ord FROM pg_catalog.pg_index i \
                      CROSS JOIN LATERAL unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
                      WHERE i.indrelid = a.attrelid AND i.indisprimary AND k.attnum = a.attnum), 0)::int4, a.attgenerated <> '' \
                     FROM pg_catalog.pg_attribute a WHERE a.attrelid = $1::int8::oid AND a.attnum > 0 \
                     AND NOT a.attisdropped ORDER BY a.attnum",
                    std::slice::from_ref(&oid),
                )?;
            let mut info = TableInfo {
                name,
                view: view != 0,
                columns: Vec::new(),
                unique: Vec::new(),
                foreign_keys: Vec::new(),
                triggers: Vec::new(),
                rules: Vec::new(),
                generated: Vec::new(),
            };
            for column in &columns {
                let name = texts(column, 0).ok_or(Refusal::Other(0))?;
                if number(column, 4) == Some(1) {
                    info.generated.push(name.clone());
                }
                info.columns.push(ColumnInfo {
                    name,
                    declared_type: texts(column, 1).unwrap_or_default(),
                    not_null: number(column, 2) == Some(1),
                    key_position: number(column, 3)
                        .and_then(|position| u32::try_from(position).ok())
                        .unwrap_or(0),
                });
            }
            let parts = |row: &Row, at: usize| -> Vec<String> {
                texts(row, at)
                    .map(|text| text.split('\u{1f}').map(str::to_owned).collect())
                    .unwrap_or_default()
            };
            for row in self.own(
                "SELECT k.conname::text, string_agg(a.attname::text, chr(31) ORDER BY c.ord) \
                 FROM pg_catalog.pg_constraint k CROSS JOIN LATERAL unnest(k.conkey) WITH ORDINALITY AS c(attnum, ord) \
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = c.attnum \
                 WHERE k.conrelid = $1::int8::oid AND k.contype = 'u' GROUP BY k.conname ORDER BY k.conname",
                std::slice::from_ref(&oid),
            )? {
                info.unique.push(parts(&row, 1));
            }
            info.unique.sort();
            for row in self.own(
                "SELECT k.conname::text, r.relname::text, k.confupdtype::text, k.confdeltype::text, \
                 string_agg(a.attname::text, chr(31) ORDER BY c.ord) \
                 FROM pg_catalog.pg_constraint k JOIN pg_catalog.pg_class r ON r.oid = k.confrelid \
                 CROSS JOIN LATERAL unnest(k.conkey) WITH ORDINALITY AS c(attnum, ord) \
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = c.attnum \
                 WHERE k.conrelid = $1::int8::oid AND k.contype = 'f' \
                 GROUP BY k.conname, r.relname, k.confupdtype, k.confdeltype ORDER BY k.conname",
                std::slice::from_ref(&oid),
            )? {
                info.foreign_keys.push(ForeignKey {
                    columns: parts(&row, 4),
                    references: texts(&row, 1).unwrap_or_default(),
                    on_update: key_rule(&texts(&row, 2).unwrap_or_default()).to_owned(),
                    on_delete: key_rule(&texts(&row, 3).unwrap_or_default()).to_owned(),
                });
            }
            for row in self.own(
                "SELECT tgname::text FROM pg_catalog.pg_trigger WHERE tgrelid = $1::int8::oid \
                 AND NOT tgisinternal ORDER BY tgname",
                std::slice::from_ref(&oid),
            )? {
                info.triggers.extend(texts(&row, 0));
            }
            for row in self.own(
                "SELECT rulename::text FROM pg_catalog.pg_rewrite WHERE ev_class = $1::int8::oid \
                 AND rulename <> '_RETURN' ORDER BY rulename",
                std::slice::from_ref(&oid),
            )? {
                info.rules.extend(texts(&row, 0));
            }
            catalog.tables.push(info);
        }
        Ok(catalog)
    }

    fn reach(
        &mut self,
        table: &str,
        _tables: usize,
    ) -> Result<Result<(usize, usize), Unseen>, Refusal> {
        // The one table, by its name as written: every name the pack sends
        // is quoted, and a quoted name is not folded. What fires on it
        // (triggers, rules, computed columns, and tables that inherit from it,
        // whose own triggers would fire), and the keys of any schema that
        // cascade from it. One query, of bounded size, whatever the store
        // holds.
        let rows = self.own(
            "WITH t AS (SELECT c.oid, c.relowner FROM pg_catalog.pg_class c \
              JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
              WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p')) \
             SELECT (SELECT count(*) FROM t), \
              (SELECT count(*) FROM t WHERE pg_catalog.pg_has_role(t.relowner, 'MEMBER')), \
              (SELECT count(*) FROM pg_catalog.pg_trigger g JOIN t ON g.tgrelid = t.oid WHERE NOT g.tgisinternal) \
              + (SELECT count(*) FROM pg_catalog.pg_rewrite r JOIN t ON r.ev_class = t.oid WHERE r.rulename <> '_RETURN') \
              + (SELECT count(*) FROM pg_catalog.pg_attribute a JOIN t ON a.attrelid = t.oid \
                 WHERE a.attgenerated <> '' AND NOT a.attisdropped) \
              + (SELECT count(*) FROM pg_catalog.pg_inherits i JOIN t ON i.inhparent = t.oid), \
              (SELECT count(*) FROM pg_catalog.pg_constraint k JOIN t ON k.confrelid = t.oid \
               WHERE k.contype = 'f' AND (k.confupdtype IN ('c', 'n', 'd') OR k.confdeltype IN ('c', 'n', 'd')))",
            &[Bound::Text(self.schema.clone()), Bound::Text(table.to_owned())],
        )?;
        let row = rows.first().ok_or(Refusal::Other(0))?;
        let count = |at: usize| number(row, at).and_then(|count| usize::try_from(count).ok());
        let (Some(found), Some(owned), Some(fires), Some(cascades)) =
            (count(0), count(1), count(2), count(3))
        else {
            return Err(Refusal::Other(0));
        };
        if found == 0 {
            return Ok(Err(Unseen::NoTable));
        }
        if owned > 0 {
            return Err(Refusal::TooPowerful);
        }
        Ok(Ok((fires, cascades)))
    }

    fn interrupter(&self) -> Interrupter {
        let token = self.client.cancel_token();
        let runtime = self.runtime.clone();
        // The server's own cancel request, sent on a new link; nothing here
        // waits for it.
        Arc::new(move || {
            let token = token.clone();
            runtime.spawn(async move {
                let _ = token.cancel_query(NoTls).await;
            });
        })
    }

    fn usable(&self) -> bool {
        !self.client.is_closed() && !self.open
    }

    fn leads_to(&self) -> Option<&Leads> {
        Some(&self.leads)
    }
}

#[cfg(test)]
mod tests {
    use super::{date_text, decode_base64, moment_text, numeric_text};

    fn numeric(count: u16, weight: i16, sign: u16, scale: u16, digits: &[u16]) -> Vec<u8> {
        let mut raw = Vec::new();
        for word in [count, weight.cast_unsigned(), sign, scale] {
            raw.extend_from_slice(&word.to_be_bytes());
        }
        for digit in digits {
            raw.extend_from_slice(&digit.to_be_bytes());
        }
        raw
    }

    #[test]
    fn a_numeric_is_read_digit_by_digit_at_its_scale() {
        // 150.5000: groups 150 and 5000, weight 0, scale 4.
        assert_eq!(
            numeric_text(&numeric(2, 0, 0, 4, &[150, 5000])).as_deref(),
            Some("150.5000")
        );
        // 12.50 with scale 2.
        assert_eq!(
            numeric_text(&numeric(2, 0, 0, 2, &[12, 5000])).as_deref(),
            Some("12.50")
        );
        // 123456789.0001: groups 1, 2345, 6789, 0001.
        assert_eq!(
            numeric_text(&numeric(4, 2, 0, 4, &[1, 2345, 6789, 1])).as_deref(),
            Some("123456789.0001")
        );
        // -0.0042: weight -1, group 42.
        assert_eq!(
            numeric_text(&numeric(1, -1, 0x4000, 4, &[42])).as_deref(),
            Some("-0.0042")
        );
        // Zero, scale 2.
        assert_eq!(
            numeric_text(&numeric(0, 0, 0, 2, &[])).as_deref(),
            Some("0.00")
        );
        // 10000: one group at weight 1, trailing groups left out.
        assert_eq!(
            numeric_text(&numeric(1, 1, 0, 0, &[1])).as_deref(),
            Some("10000")
        );
        // Not a number, and a form that does not read.
        assert_eq!(numeric_text(&numeric(0, 0, 0xC000, 0, &[])), None);
        assert_eq!(numeric_text(&numeric(2, 0, 0, 0, &[1])), None);
        assert_eq!(numeric_text(&numeric(1, 0, 0, 0, &[10_000])), None);
    }

    #[test]
    fn a_date_and_a_moment_are_read_exactly() {
        assert_eq!(date_text(0).as_deref(), Some("2000-01-01"));
        assert_eq!(date_text(9557).as_deref(), Some("2026-03-02"));
        assert_eq!(date_text(-1).as_deref(), Some("1999-12-31"));
        assert_eq!(date_text(i32::MAX), None);
        assert_eq!(moment_text(0).as_deref(), Some("2000-01-01T00:00:00Z"));
        assert_eq!(
            moment_text(825_858_300_250_000).as_deref(),
            Some("2026-03-03T13:05:00.25Z")
        );
        assert_eq!(
            moment_text(-1).as_deref(),
            Some("1999-12-31T23:59:59.999999Z")
        );
    }

    #[test]
    fn base64_reads_the_standard_alphabet_and_nothing_else() {
        assert_eq!(
            decode_base64("dXNlcjpwdw==").as_deref(),
            Some(&b"user:pw"[..])
        );
        assert_eq!(decode_base64("YQ==").as_deref(), Some(&b"a"[..]));
        assert_eq!(decode_base64("YWI=").as_deref(), Some(&b"ab"[..]));
        assert_eq!(decode_base64("YWJj").as_deref(), Some(&b"abc"[..]));
        assert_eq!(decode_base64("YWJ"), None);
        assert_eq!(decode_base64("Y*==").as_deref(), None);
    }
}
