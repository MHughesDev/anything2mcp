//! Dates, times of day, and moments as text, and a workbook's serial dates.
//!
//! A format is a closed pattern language, so that reading and writing never
//! depend on a locale. A moment is read and written through a zone given as a
//! fixed offset from UTC; nothing here reads a clock or the machine's zone.

use a2m_core::{Date, Decimal, Rounding, Timestamp};

use crate::error::Fault;

const DAY_MILLIS: i64 = 86_400_000;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// One piece of a format.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    /// `YYYY`
    Year,
    /// `MM` or `M`: two digits, or one or two.
    Month(bool),
    /// `MMM`: Jan to Dec.
    MonthName,
    /// `DD` or `D`
    Day(bool),
    /// `HH` or `H`: the hour of 24.
    Hour(bool),
    /// `hh` or `h`: the hour of 12, with `A`.
    Hour12(bool),
    /// `mm`
    Minute,
    /// `ss`
    Second,
    /// `SSS`
    Milli,
    /// `A`: AM or PM.
    Half,
    Literal(String),
}

/// What a format must describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    Date,
    Time,
    DateTime,
}

/// A checked format: `YYYY`, `MM`, `M`, `MMM`, `DD`, `D`, `HH`, `H`, `hh`,
/// `h`, `mm`, `ss`, `SSS`, `A`, and literal characters. A letter that is not
/// one of these is written between single quotes, as in `YYYY-MM-DD'T'HH:mm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Format(Vec<Part>);

impl Format {
    /// Reads a format and checks that it describes `shape`, no more and no less.
    pub(crate) fn new(pattern: &str, shape: Shape) -> Result<Self, &'static str> {
        static TOKENS: [(&str, Part); 14] = [
            ("YYYY", Part::Year),
            ("MMM", Part::MonthName),
            ("MM", Part::Month(true)),
            ("M", Part::Month(false)),
            ("DD", Part::Day(true)),
            ("D", Part::Day(false)),
            ("HH", Part::Hour(true)),
            ("H", Part::Hour(false)),
            ("hh", Part::Hour12(true)),
            ("h", Part::Hour12(false)),
            ("mm", Part::Minute),
            ("ss", Part::Second),
            ("SSS", Part::Milli),
            ("A", Part::Half),
        ];
        let mut parts = Vec::new();
        let mut rest = pattern;
        while let Some(first) = rest.chars().next() {
            if first == '\'' {
                let Some(end) = rest[1..].find('\'') else {
                    return Err("a quoted part of the format is not closed");
                };
                parts.push(Part::Literal(rest[1..=end].to_owned()));
                rest = &rest[end + 2..];
            } else if let Some((token, part)) =
                TOKENS.iter().find(|(token, _)| rest.starts_with(token))
            {
                if parts.contains(part) {
                    return Err("the format names one part twice");
                }
                parts.push(part.clone());
                rest = &rest[token.len()..];
            } else if first.is_alphabetic() {
                return Err(
                    "the format has a letter that is no part of a date; write literal letters between single quotes",
                );
            } else {
                parts.push(Part::Literal(first.to_string()));
                rest = &rest[first.len_utf8()..];
            }
        }
        // A number of one or two digits ends where the next part begins,
        // so the next part cannot be digits too: `2026111` would be the
        // first of November and the eleventh of January.
        for pair in parts.windows(2) {
            let open = matches!(
                pair[0],
                Part::Month(false) | Part::Day(false) | Part::Hour(false) | Part::Hour12(false)
            );
            let digits_follow = match &pair[1] {
                Part::Year
                | Part::Month(_)
                | Part::Day(_)
                | Part::Hour(_)
                | Part::Hour12(_)
                | Part::Minute
                | Part::Second
                | Part::Milli => true,
                Part::Literal(literal) => literal.starts_with(|c: char| c.is_ascii_digit()),
                Part::MonthName | Part::Half => false,
            };
            if open && digits_follow {
                return Err(
                    "a part of one or two digits is followed by digits, so where it ends would be a guess; put a separator between them, or write the part with two letters",
                );
            }
        }
        let has = |wanted: fn(&Part) -> bool| parts.iter().any(wanted);
        let year = has(|part| matches!(part, Part::Year));
        let month = parts
            .iter()
            .filter(|part| matches!(part, Part::Month(_) | Part::MonthName))
            .count();
        let day = parts
            .iter()
            .filter(|part| matches!(part, Part::Day(_)))
            .count();
        let hour24 = parts
            .iter()
            .filter(|part| matches!(part, Part::Hour(_)))
            .count();
        let hour12 = parts
            .iter()
            .filter(|part| matches!(part, Part::Hour12(_)))
            .count();
        let half = has(|part| matches!(part, Part::Half));
        let minute = has(|part| matches!(part, Part::Minute));
        let second = has(|part| matches!(part, Part::Second));
        let milli = has(|part| matches!(part, Part::Milli));
        let any_date = year || month > 0 || day > 0;
        let any_time = hour24 + hour12 > 0 || half || minute || second || milli;
        let whole_date = year && month == 1 && day == 1;
        let whole_time =
            hour24 + hour12 == 1 && (hour12 == 1) == half && minute && (second || !milli);
        let fits = match shape {
            Shape::Date => whole_date && !any_time,
            Shape::Time => whole_time && !any_date,
            Shape::DateTime => whole_date && whole_time,
        };
        if !fits {
            return Err(match shape {
                Shape::Date => "a date format has a year, a month, and a day, and no time",
                Shape::Time => {
                    "a time format has an hour and minutes, `A` with a 12-hour hour, and no date"
                }
                Shape::DateTime => {
                    "a date-and-time format has a year, a month, a day, an hour, and minutes"
                }
            });
        }
        Ok(Self(parts))
    }
}

#[derive(Debug, Default)]
struct Fields {
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    milli: u16,
    /// `Some(true)` for PM.
    half: Option<bool>,
}

const NOT_IN_FORMAT: Fault = Fault::unparsable("the text is not in the field type's date format");

/// Takes `least` to `most` digits from the front of `text`.
fn digits(text: &str, least: usize, most: usize) -> Result<(u16, &str), Fault> {
    let count = text
        .bytes()
        .take(most)
        .take_while(u8::is_ascii_digit)
        .count();
    if count < least {
        return Err(NOT_IN_FORMAT);
    }
    let number = text[..count].parse().map_err(|_| NOT_IN_FORMAT)?;
    Ok((number, &text[count..]))
}

fn two(text: &str, exact: bool) -> Result<(u8, &str), Fault> {
    let (number, rest) = digits(text, if exact { 2 } else { 1 }, 2)?;
    Ok((u8::try_from(number).map_err(|_| NOT_IN_FORMAT)?, rest))
}

fn read(text: &str, format: &Format) -> Result<Fields, Fault> {
    let mut fields = Fields {
        month: 1,
        day: 1,
        ..Fields::default()
    };
    let mut rest = text.trim();
    for part in &format.0 {
        rest = match part {
            Part::Year => {
                let (year, rest) = digits(rest, 4, 4)?;
                fields.year = i32::from(year);
                rest
            }
            Part::Month(exact) => {
                let (month, rest) = two(rest, *exact)?;
                fields.month = month;
                rest
            }
            Part::MonthName => {
                let name = rest.get(..3).ok_or(NOT_IN_FORMAT)?;
                let index = MONTHS
                    .iter()
                    .position(|month| month.eq_ignore_ascii_case(name))
                    .ok_or(NOT_IN_FORMAT)?;
                fields.month = u8::try_from(index + 1).map_err(|_| NOT_IN_FORMAT)?;
                &rest[3..]
            }
            Part::Day(exact) => {
                let (day, rest) = two(rest, *exact)?;
                fields.day = day;
                rest
            }
            Part::Hour(exact) | Part::Hour12(exact) => {
                let (hour, rest) = two(rest, *exact)?;
                fields.hour = hour;
                rest
            }
            Part::Minute => {
                let (minute, rest) = two(rest, true)?;
                fields.minute = minute;
                rest
            }
            Part::Second => {
                let (second, rest) = two(rest, true)?;
                fields.second = second;
                rest
            }
            Part::Milli => {
                let (milli, rest) = digits(rest, 3, 3)?;
                fields.milli = milli;
                rest
            }
            Part::Half => {
                let name = rest.get(..2).ok_or(NOT_IN_FORMAT)?;
                fields.half = Some(if name.eq_ignore_ascii_case("PM") {
                    true
                } else if name.eq_ignore_ascii_case("AM") {
                    false
                } else {
                    return Err(NOT_IN_FORMAT);
                });
                &rest[2..]
            }
            Part::Literal(literal) => rest.strip_prefix(literal.as_str()).ok_or(NOT_IN_FORMAT)?,
        };
    }
    if !rest.is_empty() {
        return Err(NOT_IN_FORMAT);
    }
    Ok(fields)
}

fn date_of(fields: &Fields) -> Result<Date, Fault> {
    Date::new(fields.year, fields.month, fields.day)
        .map_err(|_| Fault::unparsable("the text names a day that does not exist"))
}

/// Milliseconds since midnight.
fn time_of(fields: &Fields) -> Result<i64, Fault> {
    const NO_SUCH_TIME: Fault = Fault::unparsable("the text names a time that does not exist");
    let hour = match fields.half {
        None => fields.hour,
        Some(_) if fields.hour == 0 || fields.hour > 12 => return Err(NO_SUCH_TIME),
        Some(pm) => fields.hour % 12 + if pm { 12 } else { 0 },
    };
    if hour > 23 || fields.minute > 59 || fields.second > 59 {
        return Err(NO_SUCH_TIME);
    }
    Ok(
        ((i64::from(hour) * 60 + i64::from(fields.minute)) * 60 + i64::from(fields.second)) * 1000
            + i64::from(fields.milli),
    )
}

fn write(format: &Format, date: Option<Date>, time: Option<i64>) -> String {
    let time = time.unwrap_or(0);
    let hour = time / 3_600_000;
    let minute = time / 60_000 % 60;
    let second = time / 1000 % 60;
    let milli = time % 1000;
    let hour12 = if hour % 12 == 0 { 12 } else { hour % 12 };
    let (year, month, day) = date.map_or((1, 1, 1), |date| (date.year(), date.month(), date.day()));
    let mut out = String::new();
    for part in &format.0 {
        match part {
            Part::Year => out.push_str(&format!("{year:04}")),
            Part::Month(true) => out.push_str(&format!("{month:02}")),
            Part::Month(false) => out.push_str(&month.to_string()),
            Part::MonthName => {
                out.push_str(
                    MONTHS
                        .get(usize::from(month).saturating_sub(1))
                        .unwrap_or(&""),
                );
            }
            Part::Day(true) => out.push_str(&format!("{day:02}")),
            Part::Day(false) => out.push_str(&day.to_string()),
            Part::Hour(true) => out.push_str(&format!("{hour:02}")),
            Part::Hour(false) => out.push_str(&hour.to_string()),
            Part::Hour12(true) => out.push_str(&format!("{hour12:02}")),
            Part::Hour12(false) => out.push_str(&hour12.to_string()),
            Part::Minute => out.push_str(&format!("{minute:02}")),
            Part::Second => out.push_str(&format!("{second:02}")),
            Part::Milli => out.push_str(&format!("{milli:03}")),
            Part::Half => out.push_str(if hour < 12 { "AM" } else { "PM" }),
            Part::Literal(literal) => out.push_str(literal),
        }
    }
    out
}

/// A zone as a fixed offset from UTC, in minutes: `UTC`, `Z`, `+02:00`, `-05:30`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Offset(i32);

impl Offset {
    pub(crate) fn new(zone: &str) -> Result<Self, &'static str> {
        const BAD: &str =
            "a zone is UTC or a fixed offset such as +02:00; named zones are not built yet";
        if zone == "UTC" || zone == "Z" {
            return Ok(Self(0));
        }
        let bytes = zone.as_bytes();
        let shaped = bytes.len() == 6
            && (bytes[0] == b'+' || bytes[0] == b'-')
            && bytes[3] == b':'
            && [1, 2, 4, 5].iter().all(|at| bytes[*at].is_ascii_digit());
        if !shaped {
            return Err(BAD);
        }
        let hours: i32 = zone[1..3].parse().map_err(|_| BAD)?;
        let minutes: i32 = zone[4..6].parse().map_err(|_| BAD)?;
        if hours > 14 || minutes > 59 || (hours == 14 && minutes > 0) {
            return Err(BAD);
        }
        let total = hours * 60 + minutes;
        Ok(Self(if bytes[0] == b'-' { -total } else { total }))
    }

    fn millis(self) -> i64 {
        i64::from(self.0) * 60_000
    }
}

/// Reads a date written in `format`.
pub(crate) fn date_from_text(text: &str, format: &Format) -> Result<Date, Fault> {
    date_of(&read(text, format)?)
}

/// Writes a date in `format`.
pub(crate) fn text_from_date(date: Date, format: &Format) -> String {
    write(format, Some(date), None)
}

/// Reads a time of day written in `format`, as milliseconds since midnight.
pub(crate) fn time_from_text(text: &str, format: &Format) -> Result<i64, Fault> {
    time_of(&read(text, format)?)
}

/// Reads a moment written in `format` as the wall time of `zone`.
pub(crate) fn instant_from_text(
    text: &str,
    format: &Format,
    zone: Offset,
) -> Result<Timestamp, Fault> {
    let fields = read(text, format)?;
    let days = i64::from(date_of(&fields)?.to_days());
    let millis = days * DAY_MILLIS + time_of(&fields)? - zone.millis();
    Ok(Timestamp::from_unix_millis(millis))
}

/// Writes a moment in `format`, as the wall time of `zone`.
pub(crate) fn text_from_instant(
    moment: Timestamp,
    format: &Format,
    zone: Offset,
) -> Result<String, Fault> {
    const BEYOND: Fault = Fault::range("the moment lies outside the years 1 to 9999");
    let local = moment
        .as_unix_millis()
        .checked_add(zone.millis())
        .ok_or(BEYOND)?;
    let days = i32::try_from(local.div_euclid(DAY_MILLIS)).map_err(|_| BEYOND)?;
    let date = Date::from_days(days).map_err(|_| BEYOND)?;
    Ok(write(
        format,
        Some(date),
        Some(local.rem_euclid(DAY_MILLIS)),
    ))
}

/// A workbook's date system: which day its serial numbers count from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSystem {
    /// Serial 1 is 1900-01-01, and serial 60 is a 29 February 1900 that never was.
    From1900,
    /// Serial 0 is 1904-01-01.
    From1904,
}

impl DateSystem {
    /// The name used in a compiled call: `"1900"` or `"1904"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::From1900 => "1900",
            Self::From1904 => "1904",
        }
    }

    /// The date system with that name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "1900" => Some(Self::From1900),
            "1904" => Some(Self::From1904),
            _ => None,
        }
    }
}

/// The date a workbook's serial number stands for. A fraction is a time of
/// day and is dropped.
pub(crate) fn date_from_serial(serial: &Decimal, system: DateSystem) -> Result<Date, Fault> {
    const BEFORE: Fault = Fault::range("the serial date lies before the workbook's first day");
    const BEYOND: Fault = Fault::range("the serial date lies beyond the year 9999");
    const NEVER: Fault = Fault::range("serial 60 is a 29 February 1900, a day that never was");
    let day = serial
        .round(0, Rounding::Floor)
        .to_i64()
        .and_then(|day| i32::try_from(day).ok())
        .ok_or(BEYOND)?;
    // Days from 1970-01-01 to 1899-12-30 and to 1904-01-01.
    const DAY_ZERO_1900: i32 = -25_569;
    const DAY_ZERO_1904: i32 = -24_107;
    let days = match system {
        DateSystem::From1900 if day < 1 => return Err(BEFORE),
        DateSystem::From1900 if day == 60 => return Err(NEVER),
        // Before the day that never was, the count is one short.
        DateSystem::From1900 if day < 60 => DAY_ZERO_1900 + 1 + day,
        DateSystem::From1900 => DAY_ZERO_1900.checked_add(day).ok_or(BEYOND)?,
        DateSystem::From1904 if day < 0 => return Err(BEFORE),
        DateSystem::From1904 => DAY_ZERO_1904.checked_add(day).ok_or(BEYOND)?,
    };
    Date::from_days(days).map_err(|_| BEYOND)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{VALUE_OUT_OF_RANGE, VALUE_UNPARSABLE};

    fn date(year: i32, month: u8, day: u8) -> Date {
        Date::new(year, month, day).unwrap()
    }

    fn d(text: &str) -> Decimal {
        Decimal::parse(text).unwrap()
    }

    #[test]
    fn dates_are_read_and_written_in_a_format() {
        for (pattern, text, want) in [
            ("YYYY-MM-DD", "2026-11-17", date(2026, 11, 17)),
            ("MM/DD/YYYY", "02/29/2024", date(2024, 2, 29)),
            ("M/D/YYYY", "2/9/2024", date(2024, 2, 9)),
            ("D.M.YYYY", "17.11.2026", date(2026, 11, 17)),
            ("DD MMM YYYY", "07 Mar 2025", date(2025, 3, 7)),
            ("YYYYMMDD", "20261117", date(2026, 11, 17)),
        ] {
            let format = Format::new(pattern, Shape::Date).unwrap();
            assert_eq!(date_from_text(text, &format).unwrap(), want, "{text}");
            assert_eq!(text_from_date(want, &format), text, "{text}");
        }
        let format = Format::new("DD MMM YYYY", Shape::Date).unwrap();
        assert_eq!(
            date_from_text(" 07 mar 2025 ", &format).unwrap(),
            date(2025, 3, 7)
        );
    }

    #[test]
    fn a_date_is_never_guessed() {
        let format = Format::new("MM/DD/YYYY", Shape::Date).unwrap();
        for text in [
            "",
            "2/9/2024",
            "02/30/2024",
            "13/01/2024",
            "02/29/2023",
            "02-29-2024",
            "02/29/2024 10:00",
            "02/29/24",
            "aa/bb/cccc",
            "02/29/2024x",
        ] {
            assert_eq!(
                date_from_text(text, &format).unwrap_err().code,
                VALUE_UNPARSABLE,
                "{text}"
            );
        }
    }

    #[test]
    fn a_format_is_a_closed_language() {
        for (pattern, shape) in [
            ("YYYY-MM", Shape::Date),
            ("YY-MM-DD", Shape::Date),
            ("yyyy-MM-dd", Shape::Date),
            ("YYYY-MM-DD HH:mm", Shape::Date),
            ("YYYY-MM-DDTHH:mm", Shape::DateTime),
            ("YYYY-MM-DD 'at HH:mm", Shape::DateTime),
            ("YYYY-MM-DD MM", Shape::Date),
            ("hh:mm", Shape::Time),
            ("HH:mm A", Shape::Time),
            ("HH", Shape::Time),
            ("HH:mm:SSS", Shape::Time),
            ("YYYY-MM-DD", Shape::DateTime),
            ("YYYY-MM-DD HH:mm", Shape::Time),
        ] {
            assert!(Format::new(pattern, shape).is_err(), "{pattern}");
        }
    }

    #[test]
    fn a_format_whose_parts_run_together_is_refused_because_reading_it_would_be_a_guess() {
        // `2026111` would be the first of November and the eleventh of
        // January alike.
        for (pattern, shape) in [
            ("YYYYMD", Shape::Date),
            ("MD/YYYY", Shape::Date),
            ("MDD/YYYY", Shape::Date),
            ("DMYYYY", Shape::Date),
            ("D'1'M-YYYY", Shape::Date),
            ("Hmm", Shape::Time),
            ("hmm A", Shape::Time),
            ("YYYY-MM-DHH:mm", Shape::DateTime),
        ] {
            let problem = Format::new(pattern, shape).unwrap_err();
            assert!(problem.contains("guess"), "{pattern}: {problem}");
        }
        // Parts of a fixed width may run together, and a part of one or two
        // digits may end the text or stand before a separator or a name.
        for (pattern, shape) in [
            ("YYYYMMDD", Shape::Date),
            ("YYYYMMD", Shape::Date),
            ("M/D/YYYY", Shape::Date),
            ("D MMM YYYY", Shape::Date),
            ("DMMMYYYY", Shape::Date),
            ("HHmm", Shape::Time),
            ("hA:mm", Shape::Time),
            ("H:mm", Shape::Time),
        ] {
            assert!(Format::new(pattern, shape).is_ok(), "{pattern}");
        }
        let format = Format::new("YYYYMMD", Shape::Date).unwrap();
        assert_eq!(date_from_text("2026111", &format).unwrap(), date(2026, 11, 1));
        assert_eq!(text_from_date(date(2026, 1, 11), &format), "20260111");
    }

    #[test]
    fn times_of_day_are_read() {
        let day = Format::new("HH:mm:ss", Shape::Time).unwrap();
        assert_eq!(time_from_text("13:05:09", &day).unwrap(), 47_109_000);
        let half = Format::new("h:mm A", Shape::Time).unwrap();
        assert_eq!(time_from_text("12:00 AM", &half).unwrap(), 0);
        assert_eq!(time_from_text("12:30 pm", &half).unwrap(), 45_000_000);
        assert_eq!(time_from_text("1:05 PM", &half).unwrap(), 47_100_000);
        for (format, text) in [
            (&day, "24:00:00"),
            (&day, "12:60:00"),
            (&half, "13:00 PM"),
            (&half, "0:00 AM"),
        ] {
            assert_eq!(
                time_from_text(text, format).unwrap_err().code,
                VALUE_UNPARSABLE,
                "{text}"
            );
        }
    }

    #[test]
    fn a_moment_is_read_and_written_through_a_given_zone() {
        let format = Format::new("YYYY-MM-DD'T'HH:mm:ss.SSS", Shape::DateTime).unwrap();
        let utc = Offset::new("UTC").unwrap();
        let east = Offset::new("+02:00").unwrap();
        let west = Offset::new("-05:30").unwrap();
        let moment = instant_from_text("2026-11-17T12:00:00.250", &format, utc).unwrap();
        assert_eq!(moment.as_unix_millis(), 1_794_916_800_250);
        // The same wall time further east is an earlier moment.
        let earlier = instant_from_text("2026-11-17T12:00:00.250", &format, east).unwrap();
        assert_eq!(
            moment.as_unix_millis() - earlier.as_unix_millis(),
            7_200_000
        );
        assert_eq!(
            text_from_instant(moment, &format, east).unwrap(),
            "2026-11-17T14:00:00.250"
        );
        assert_eq!(
            text_from_instant(moment, &format, west).unwrap(),
            "2026-11-17T06:30:00.250"
        );
        // A moment before 1970, and one that crosses midnight in its zone.
        let old = instant_from_text("1969-12-31T23:00:00.000", &format, utc).unwrap();
        assert_eq!(old.as_unix_millis(), -3_600_000);
        assert_eq!(
            text_from_instant(old, &format, east).unwrap(),
            "1970-01-01T01:00:00.000"
        );
        let half = Format::new("M/D/YYYY h:mm A", Shape::DateTime).unwrap();
        assert_eq!(
            text_from_instant(moment, &half, utc).unwrap(),
            "11/17/2026 12:00 PM"
        );
        assert_eq!(
            text_from_instant(Timestamp::from_unix_millis(i64::MAX), &format, utc)
                .unwrap_err()
                .code,
            VALUE_OUT_OF_RANGE
        );
    }

    #[test]
    fn a_zone_is_a_fixed_offset() {
        for zone in ["Europe/Paris", "+2", "+02:60", "+15:00", "02:00", "utc", ""] {
            assert!(Offset::new(zone).is_err(), "{zone}");
        }
        assert_eq!(Offset::new("Z").unwrap(), Offset::new("+00:00").unwrap());
        assert_eq!(Offset::new("-14:00").unwrap().millis(), -50_400_000);
    }

    #[test]
    fn serial_dates_follow_the_workbooks_date_system() {
        let old = DateSystem::From1900;
        let new = DateSystem::From1904;
        for (serial, system, want) in [
            ("1", old, date(1900, 1, 1)),
            ("59", old, date(1900, 2, 28)),
            ("61", old, date(1900, 3, 1)),
            ("25569", old, date(1970, 1, 1)),
            ("46343", old, date(2026, 11, 17)),
            ("46343.75", old, date(2026, 11, 17)),
            ("0", new, date(1904, 1, 1)),
            ("44881", new, date(2026, 11, 17)),
        ] {
            assert_eq!(
                date_from_serial(&d(serial), system).unwrap(),
                want,
                "{serial}"
            );
        }
        for (serial, system) in [
            ("0", old),
            ("-1", old),
            ("60", old),
            ("-1", new),
            ("99999999", old),
            ("0.5", old),
        ] {
            assert_eq!(
                date_from_serial(&d(serial), system).unwrap_err().code,
                VALUE_OUT_OF_RANGE,
                "{serial}"
            );
        }
    }
}
