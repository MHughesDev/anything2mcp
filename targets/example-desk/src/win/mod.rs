//! The stand on Windows: a window of plain Win32 controls in a process of
//! its own, which the test starts and stops.
//!
//! The process is the test's own program, copied into a folder of the
//! session's own under the name `example-desk.exe` and started again with
//! one test alone: the one [`stand_child!`](crate::stand_child) declares.
//! So the program the desktop pack must prove is a file the test made,
//! that no other process runs, and whose name is the stand's. The copy
//! needs no build step of its own and is never stale.
//!
//! One session at a time in a test process: the tests of one binary run in
//! parallel, and two windows of one program would be two applications to
//! the pack. A [`Session`] holds the seat until it and its stand are gone.
//!
//! The control interface is a line protocol on the process's standard
//! input and output, every answer marked with [`PREFIX`]; the test
//! harness's own lines on the same output are passed over.

mod window;

use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::{Line, Order, REQUIRE_ENV, StandError, Truth, required};

/// The variable that makes a started copy of the test serve the stand.
const CHILD_ENV: &str = "A2M_DESK_STAND_CHILD";
/// The variable that carries the order the window shows.
const ORDER_ENV: &str = "A2M_DESK_STAND_ORDER";
/// The name of the test, in every test binary that starts a stand, that
/// serves it in the started copy.
pub const CHILD_TEST: &str = "example_desk_stand_child";
/// The mark of the stand's own lines on its output.
const PREFIX: &str = "example-desk: ";
/// The name of the stand's program, as the desktop pack reads it.
const PROGRAM: &str = "example-desk.exe";
/// How long the stand may take to show its window.
const START_LIMIT: Duration = Duration::from_secs(30);
/// How long one question to the control interface may take.
const ASK_LIMIT: Duration = Duration::from_secs(10);
/// How long a session waits for the seat.
const SEAT_LIMIT: Duration = Duration::from_secs(300);
/// The most of the stand's error output a failure quotes.
const STDERR_KEPT: usize = 4_000;

/// Serves the stand and ends the process, when this process is a started
/// copy; otherwise returns at once.
pub fn serve_if_child() {
    if std::env::var(CHILD_ENV).is_ok_and(|value| value == "1") {
        let code = window::serve();
        std::process::exit(code);
    }
}

fn encode_order(order: &Order) -> String {
    let mut parts = vec![
        order.number.clone(),
        order.customer.clone(),
        order.status.clone(),
    ];
    for line in &order.lines {
        parts.push(format!(
            "{}\u{1f}{}\u{1f}{}",
            line.code, line.description, line.quantity
        ));
    }
    parts.join("\u{1e}")
}

fn decode_order(text: &str) -> Option<Order> {
    let mut parts = text.split('\u{1e}');
    let number = parts.next()?.to_owned();
    let customer = parts.next()?.to_owned();
    let status = parts.next()?.to_owned();
    let mut lines = Vec::new();
    for part in parts {
        let mut cells = part.split('\u{1f}');
        lines.push(Line {
            code: cells.next()?.to_owned(),
            description: cells.next()?.to_owned(),
            quantity: cells.next()?.to_owned(),
        });
    }
    Some(Order {
        number,
        customer,
        status,
        lines,
    })
}

/// The one seat of this test process.
static SEAT: Mutex<bool> = Mutex::new(false);
static SEAT_FREED: Condvar = Condvar::new();

/// What a session and its stand share: the seat and the folder with the
/// program's copy, both given back when the last of them is gone.
struct Held {
    folder: PathBuf,
}

impl Held {
    fn take(folder: PathBuf) -> Result<Self, StandError> {
        let start = Instant::now();
        let mut busy = SEAT.lock().unwrap_or_else(PoisonError::into_inner);
        while *busy {
            let left = SEAT_LIMIT.saturating_sub(start.elapsed());
            if left.is_zero() {
                return Err(StandError(format!(
                    "another test of this process held the desktop for {} seconds",
                    SEAT_LIMIT.as_secs()
                )));
            }
            busy = SEAT_FREED
                .wait_timeout(busy, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *busy = true;
        Ok(Self { folder })
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
        *SEAT.lock().unwrap_or_else(PoisonError::into_inner) = false;
        SEAT_FREED.notify_one();
    }
}

/// Whether this process's window station is the interactive one: whether
/// a window it shows is on a desktop UI Automation reads. A service, or a
/// runner with no signed-in session, has none.
#[allow(unsafe_code)] // two calls into the system, explained below
fn interactive() -> Result<bool, String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::StationsAndDesktops::{
        GetProcessWindowStation, GetUserObjectInformationW, UOI_FLAGS, USEROBJECTFLAGS,
    };
    use windows::Win32::UI::WindowsAndMessaging::WSF_VISIBLE;
    // SAFETY: no argument; the handle the call gives is not to be closed.
    let station = unsafe { GetProcessWindowStation() }
        .map_err(|error| format!("GetProcessWindowStation: {}", error.message()))?;
    let mut flags = USEROBJECTFLAGS::default();
    let mut needed = 0_u32;
    // SAFETY: `flags` is ours and as long as the length says.
    unsafe {
        GetUserObjectInformationW(
            HANDLE(station.0),
            UOI_FLAGS,
            Some((&raw mut flags).cast()),
            size_of::<USEROBJECTFLAGS>() as u32,
            Some(&raw mut needed),
        )
    }
    .map_err(|error| format!("GetUserObjectInformationW: {}", error.message()))?;
    Ok(flags.dwFlags & WSF_VISIBLE as u32 != 0)
}

/// A desktop for one test: the seat of this test process, and a copy of
/// the test's program to run the stand. Dropping it, and its stand, gives
/// both back.
pub struct Session {
    held: Arc<Held>,
    program: PathBuf,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// A desktop for one test, or `None` when this process has no
    /// interactive desktop.
    ///
    /// # Errors
    ///
    /// When the program cannot be copied, the seat is not freed in time,
    /// or there is no interactive desktop and `A2M_REQUIRE_DESKTOP=1` says
    /// there must be.
    pub fn start() -> Result<Option<Self>, StandError> {
        let shown = interactive().map_err(StandError)?;
        if !shown {
            if required() {
                return Err(StandError(format!(
                    "this process's window station is not the interactive one, so no window it shows can be read, and {REQUIRE_ENV}=1 says one must be"
                )));
            }
            return Ok(None);
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let folder = std::env::temp_dir().join(format!(
            "example-desk-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let held = Arc::new(Held::take(folder.clone())?);
        let made = |doing: &str| {
            let doing = doing.to_owned();
            move |error: std::io::Error| StandError(format!("{doing}: {error}"))
        };
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).map_err(made("making the session's folder"))?;
        let own = std::env::current_exe().map_err(made("finding the test's program"))?;
        let program = folder.join(PROGRAM);
        std::fs::copy(&own, &program).map_err(made("copying the test's program"))?;
        let program =
            std::fs::canonicalize(&program).map_err(made("resolving the program's copy"))?;
        Ok(Some(Self { held, program }))
    }

    /// No bus: UI Automation reads the session the process runs in.
    #[must_use]
    pub fn bus(&self) -> Option<&str> {
        None
    }

    /// The program the stand runs, by its absolute path: a surface names
    /// it.
    #[must_use]
    pub fn program(&self) -> String {
        self.program.to_string_lossy().into_owned()
    }
}

/// The stand's process and its control interface.
pub(crate) struct Stand {
    child: Child,
    input: Mutex<ChildStdin>,
    answers: Mutex<Receiver<String>>,
    errors: Arc<Mutex<String>>,
    /// Save and the password edit, as window handles in number form.
    pub(crate) save: isize,
    pub(crate) password: isize,
    _held: Arc<Held>,
}

fn tail(errors: &Mutex<String>) -> String {
    let said = errors.lock().unwrap_or_else(PoisonError::into_inner);
    let start = said.len().saturating_sub(STDERR_KEPT);
    let start = (start..said.len())
        .find(|at| said.is_char_boundary(*at))
        .unwrap_or(said.len());
    said[start..].trim().to_owned()
}

impl Stand {
    pub(crate) async fn start(session: &Session, order: Order) -> Result<Self, StandError> {
        let held = Arc::clone(&session.held);
        let program = session.program.clone();
        tokio::task::spawn_blocking(move || Self::start_blocking(held, &program, &order))
            .await
            .map_err(|error| StandError(format!("the start of the stand stopped: {error}")))?
    }

    fn start_blocking(held: Arc<Held>, program: &Path, order: &Order) -> Result<Self, StandError> {
        use std::os::windows::process::CommandExt;
        /// No console window for the stand's process.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut child = Command::new(program)
            .args([CHILD_TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD_ENV, "1")
            .env(ORDER_ENV, encode_order(order))
            .env_remove(REQUIRE_ENV)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| {
                StandError(format!(
                    "starting the stand's program {}: {error}",
                    program.display()
                ))
            })?;
        let (Some(input), Some(output), Some(mut error_output)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(StandError("the stand's pipes were not made".to_owned()));
        };
        let (send, answers) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if let Some(answer) = line.strip_prefix(PREFIX)
                    && send.send(answer.to_owned()).is_err()
                {
                    break;
                }
            }
        });
        let errors = Arc::new(Mutex::new(String::new()));
        let kept = Arc::clone(&errors);
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 1024];
            while let Ok(read) = error_output.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                let mut said = kept.lock().unwrap_or_else(PoisonError::into_inner);
                said.push_str(&String::from_utf8_lossy(&buffer[..read]));
                if said.len() > 4 * STDERR_KEPT {
                    let cut = said.len() - 2 * STDERR_KEPT;
                    let cut = (cut..said.len())
                        .find(|at| said.is_char_boundary(*at))
                        .unwrap_or(said.len());
                    said.drain(..cut);
                }
            }
        });
        let ready = answers.recv_timeout(START_LIMIT);
        let fields: Vec<String> = match &ready {
            Ok(line) => line.split('\t').map(str::to_owned).collect(),
            Err(_) => Vec::new(),
        };
        let handle = |at: usize| fields.get(at).and_then(|text| text.parse::<isize>().ok());
        let (Some("ready"), Some(save), Some(password)) =
            (fields.first().map(String::as_str), handle(2), handle(3))
        else {
            let _ = child.kill();
            let status = child.wait();
            let why = match ready {
                Ok(line) => format!("it said `{line}` instead of `ready`"),
                Err(RecvTimeoutError::Timeout) => format!(
                    "it did not show its window within {} seconds",
                    START_LIMIT.as_secs()
                ),
                Err(RecvTimeoutError::Disconnected) => format!(
                    "it ended ({status:?}) before it showed its window; a test binary that starts the stand must declare `example_desk::stand_child!()`"
                ),
            };
            return Err(StandError(format!(
                "the stand's process {}: {why}. Its error output: {}",
                program.display(),
                tail(&errors)
            )));
        };
        Ok(Self {
            child,
            input: Mutex::new(input),
            answers: Mutex::new(answers),
            errors,
            save,
            password,
            _held: held,
        })
    }

    /// Asks the control interface one thing and waits for its answer, the
    /// line that starts with `kind`.
    fn ask(&self, line: &str, kind: &str) -> Result<String, StandError> {
        let answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
        {
            let mut input = self.input.lock().unwrap_or_else(PoisonError::into_inner);
            writeln!(input, "{line}")
                .and_then(|()| input.flush())
                .map_err(|error| {
                    StandError(format!(
                        "the stand's control interface took no `{line}`: {error}. Its error output: {}",
                        tail(&self.errors)
                    ))
                })?;
        }
        let start = Instant::now();
        loop {
            let left = ASK_LIMIT.saturating_sub(start.elapsed());
            match answers.recv_timeout(left) {
                Ok(answer) if answer == kind || answer.starts_with(&format!("{kind}\t")) => {
                    return Ok(answer);
                }
                Ok(_) => {}
                Err(error) => {
                    return Err(StandError(format!(
                        "the stand's control interface did not answer `{line}` ({error}). Its error output: {}",
                        tail(&self.errors)
                    )));
                }
            }
        }
    }

    pub(crate) fn truth(&self) -> Result<Truth, StandError> {
        let answer = self.ask("truth", "truth")?;
        let fields: Vec<&str> = answer.split('\t').collect();
        let number = |at: usize| fields.get(at).and_then(|text| text.parse::<u64>().ok());
        let (Some(writes), Some(secret_reads), Some(tracking), Some(asked)) =
            (number(1), number(2), fields.get(3), fields.get(4))
        else {
            return Err(StandError(format!(
                "the stand's answer `{answer}` is not in the form it writes"
            )));
        };
        let asked = asked
            .split(';')
            .filter(|pair| !pair.is_empty())
            .filter_map(|pair| {
                let (member, times) = pair.rsplit_once('=')?;
                Some((member.to_owned(), times.parse::<u64>().ok()?))
            })
            .collect();
        Ok(Truth {
            writes,
            secret_reads,
            asked,
            tracking: (*tracking).to_owned(),
        })
    }

    pub(crate) fn show_late(&self, after: Duration) -> Result<(), StandError> {
        self.ask(&format!("late {}", after.as_millis()), "late")
            .map(|_| ())
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        if let Ok(mut input) = self.input.lock() {
            let _ = writeln!(input, "quit");
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
