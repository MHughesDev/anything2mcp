//! Lists the migration files, so that a crate adds a migration by adding one
//! file to `migrations/` and edits no code here (decision 0080).
//!
//! A file is named `NNNN-title.sql`. The number is its place in the one
//! sequence; two files with one number stop the build.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let folder = Path::new(&env::var("CARGO_MANIFEST_DIR")?).join("migrations");
    println!("cargo:rerun-if-changed=migrations");

    let mut files: BTreeMap<u32, String> = BTreeMap::new();
    for entry in fs::read_dir(&folder)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".sql") else {
            continue;
        };
        let number: u32 = stem
            .split('-')
            .next()
            .filter(|digits| digits.len() == 4)
            .and_then(|digits| digits.parse().ok())
            .ok_or_else(|| format!("migration `{name}` is not named `NNNN-title.sql`"))?;
        if let Some(other) = files.insert(number, name.clone()) {
            return Err(format!("migrations `{other}` and `{name}` share one number").into());
        }
    }

    let mut code = String::from("&[\n");
    for (number, name) in &files {
        writeln!(
            code,
            "    ({number}, {name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/migrations/\", {name:?}))),"
        )?;
    }
    code.push_str("]\n");
    fs::write(Path::new(&env::var("OUT_DIR")?).join("migrations.rs"), code)?;
    Ok(())
}
