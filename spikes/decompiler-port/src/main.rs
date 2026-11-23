//! Dumps what the Rust port of the decompiler finds in a program: every function
//! (address, name, source) and the C text for those whose name holds a filter.
//!
//! usage: dump <program> <loader|full> <filter> <out-file>
//!
//! Output: one `F<TAB>address<TAB>name<TAB>source` line per function, then for
//! each decompiled one `D<TAB>address<TAB>name<TAB>lines` followed by the text.
use std::fmt::Write as _;
use std::io::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use ghidra_decompiler::program::{Program, SymbolLoading};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: dump <program> <loader|full> <filter> <out-file>");
        std::process::exit(2);
    }
    let loading = if args[2] == "loader" { SymbolLoading::Loader } else { SymbolLoading::Full };
    let filter = &args[3];
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[4]).expect("create out"));
    let mut program = Program::open_with(Path::new(&args[1]), None, loading).expect("open");
    writeln!(out, "L\t{}", program.language_id()).ok();
    let functions = program.functions().expect("functions");
    for f in &functions {
        writeln!(out, "F\t{:#x}\t{}\t{:?}", f.address, f.name, f.source).ok();
    }
    out.flush().ok();
    // A filter that starts with `@` names a file of addresses, one per line (hex, `0x` prefix).
    let wanted: Option<std::collections::HashSet<String>> = filter
        .strip_prefix('@')
        .map(|p| std::fs::read_to_string(p).expect("address file").lines().map(|l| l.trim().to_string()).collect());
    for f in &functions {
        // A filter that starts with `^` matches the start of the name; otherwise a substring.
        let keep = if let Some(w) = &wanted {
            w.contains(&format!("{:#x}", f.address))
        } else {
            match filter.strip_prefix('^') {
                Some(prefix) => f.name.starts_with(prefix),
                None => filter.is_empty() || f.name.contains(filter.as_str()),
            }
        };
        if !keep {
            continue;
        }
        let started = std::time::Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| program.decompile(f.address)));
        let text = match result {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => format!("//ERROR {e}\n"),
            Err(_) => "//PANIC\n".to_string(),
        };
        let mut head = String::new();
        write!(head, "D\t{:#x}\t{}\t{}\t{}ms", f.address, f.name, text.lines().count(), started.elapsed().as_millis()).ok();
        writeln!(out, "{head}").ok();
        out.write_all(text.as_bytes()).ok();
        if !text.ends_with('\n') {
            writeln!(out).ok();
        }
        writeln!(out, "E").ok();
        out.flush().ok();
    }
}
