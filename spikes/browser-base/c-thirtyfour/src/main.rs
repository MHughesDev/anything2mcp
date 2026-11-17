//! Throwaway spike code. Candidate (c): a WebDriver client, the `thirtyfour` crate, 0.38.3.
//!
//! WebDriver needs a second program, the driver, between us and the browser,
//! and the driver must match the browser's major version. On this machine the
//! driver found is for another version, so this program stops at the session.
//! Everything after `WebDriver::new` is written but was never run.

use std::process::Stdio;
use std::time::{Duration, Instant};

use spike_browser_common as common;
use thirtyfour::prelude::*;
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main]
async fn main() {
    let app = common::TestApp::start().await.expect("the test application starts");
    let dir = common::owned_data_dir("webdriver");
    let driver_program = std::env::var("A2M_SPIKE_DRIVER").expect("set A2M_SPIKE_DRIVER to a driver program");
    // The driver listens on a loopback port. It has no pipe and no sign-in of its own.
    let port = std::net::TcpListener::bind(("127.0.0.1", 0)).and_then(|l| l.local_addr()).expect("a free port").port();

    let t0 = Instant::now();
    let mut driver_process = tokio::process::Command::new(driver_program)
        .arg(format!("--port={port}"))
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("the driver starts");
    let mut lines = BufReader::new(driver_process.stdout.take().expect("output")).lines();
    while let Some(line) = lines.next_line().await.expect("the driver's output") {
        if line.contains("started successfully") {
            break;
        }
    }
    println!("driver ready after {} ms", common::ms(t0.elapsed()));

    let mut caps = DesiredCapabilities::chrome();
    caps.set_binary(&common::browser_program().display().to_string()).expect("binary");
    for arg in common::base_args(&dir) {
        caps.add_arg(&arg).expect("argument");
    }
    let driver = match WebDriver::new(format!("http://127.0.0.1:{port}"), caps).await {
        Ok(driver) => driver,
        Err(error) => {
            println!("no session: {error}");
            common::remove_data_dir(&dir);
            return;
        }
    };

    // Never run: the session above is refused here.
    driver.goto(app.url(common::FORM_PATH)).await.expect("page");
    // The crate's wait is a poll: it asks again on an interval until the element is there.
    let field = driver
        .query(By::Id(common::FIELD_ID))
        .wait(Duration::from_secs(10), Duration::from_millis(50))
        .first()
        .await
        .expect("field");
    println!("value: {:?} after {} ms", field.prop("value").await.expect("read"), common::ms(t0.elapsed()));
    // The classic protocol has no accessibility tree and no request interception.
    println!("page source: {} bytes", driver.source().await.expect("source").len());
    driver.quit().await.expect("quit");
    let _ = driver_process.kill().await;
    println!("left over: {:?}", common::wait_no_leftovers(&dir, Duration::from_secs(5)));
    common::remove_data_dir(&dir);
}
