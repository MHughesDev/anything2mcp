//! The product's end-to-end tests, in one test program (decision 0446).
//!
//! Each file beside this one is a module of it, so that the whole product
//! is linked once for all of them, not once for each. A test's name is its
//! module's and its own, as `faults::a_test`; run one file's tests with that
//! module's name and `::` as the filter, as
//! `cargo test -p anything2mcp --test product -- faults::`. Cargo finds no
//! test program of its own in this folder (`autotests = false`), so a new
//! file is a module here, and the test at the end of this file fails while
//! one is not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

mod approval_surface;
mod asserted_publish;
mod capture;
mod capture_use;
mod command;
mod command_write;
mod completion;
mod copied_data_folder;
mod data_write;
mod engine_log;
mod expired_session;
mod export_download;
mod faults;
mod file_write;
mod first_hour;
mod first_hour_faults;
mod first_hour_signed_in;
mod first_read;
mod first_run;
mod forgery_write;
mod home;
mod local_engine;
mod net_tools;
mod normalized_write;
mod other_forms;
mod other_packs;
mod page_http_faults;
mod publish_launcher;
mod publish_page;
mod random_kill;
mod ready;
mod renewal;
mod runtime_folder;
mod scout_files;
mod screen_drift;
mod serve_stop;
mod shadow;
mod shared_session;
mod short_path;
mod sign_in_detect;
mod sign_in_signature;
mod signed_in_read;
mod signed_in_web_read;
mod site_tools;
mod start_on_demand;
mod stream_write;
mod target_add;
mod trust_roots;
mod vault_restart;
mod verified_write;
mod verify_after_loss;
mod watch;
mod web_draft;

/// Every file in this folder is a module of this program: with the
/// folder's test programs not found by cargo, a file left out would build
/// and run nothing, and say nothing.
#[test]
fn every_test_file_is_a_module_of_this_program() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let main = std::fs::read_to_string(folder.join("main.rs")).unwrap();
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(&folder).unwrap() {
        let path = entry.unwrap().path();
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if path.extension().is_some_and(|extension| extension == "rs")
            && stem != "main"
            && !main.contains(&format!("\nmod {stem};\n"))
        {
            missing.push(stem.to_owned());
        }
    }
    assert!(
        missing.is_empty(),
        "test files that are not a module of tests/main.rs: {missing:?}"
    );
}
