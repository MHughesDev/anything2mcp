//! Every compiled target a release kept still loads with this build
//! (decision 0437, point 2): for each folder `contracts/<version>/`, each
//! compiled target that `compiled-targets.json` lists decodes with today's
//! `a2m-core` against the hash kept beside it, and encodes back to the same
//! bytes, so that nothing it held is lost on the way in.
//!
//! `A2M_CONTRACTS` names another folder of kept contracts, for a check of
//! one not yet committed; `tools/contracts.py` says how a folder is written.

use std::path::{Path, PathBuf};

use a2m_core::{CompiledTarget, Digest};

/// The folder of kept contracts.
fn contracts() -> PathBuf {
    std::env::var_os("A2M_CONTRACTS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts"),
        PathBuf::from,
    )
}

/// Every way the compiled targets kept in `folder` fail to load, as
/// sentences; empty when they all load.
fn problems_of(folder: &Path) -> Vec<String> {
    let list = folder.join("compiled-targets.json");
    let Ok(text) = std::fs::read_to_string(&list) else {
        return vec![format!("{} cannot be read", list.display())];
    };
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return vec![format!("{} is not a list", list.display())];
    };
    if entries.is_empty() {
        return vec![format!("{} lists no compiled target", list.display())];
    }
    let mut problems = Vec::new();
    for entry in entries {
        let (Some(file), Some(hash)) = (
            entry.get("file").and_then(serde_json::Value::as_str),
            entry.get("hash").and_then(serde_json::Value::as_str),
        ) else {
            problems.push(format!("{}: an entry has no file or hash", list.display()));
            continue;
        };
        let path = folder.join(file);
        let Ok(bytes) = std::fs::read(&path) else {
            problems.push(format!("{} cannot be read", path.display()));
            continue;
        };
        let Ok(hash) = hash.parse::<Digest>() else {
            problems.push(format!("{}: `{hash}` is not a hash", path.display()));
            continue;
        };
        match CompiledTarget::decode(&bytes, &hash) {
            Err(failure) => {
                problems.push(format!("{} no longer loads: {failure:?}", path.display()))
            }
            Ok(decoded) => match decoded.encode() {
                Ok(again) if again.bytes == bytes => {}
                Ok(again) => problems.push(format!(
                    "{} loads, and encodes again with another hash, {}",
                    path.display(),
                    again.hash
                )),
                Err(failure) => problems.push(format!(
                    "{} loads and cannot be encoded again: {failure:?}",
                    path.display()
                )),
            },
        }
    }
    problems
}

#[test]
fn every_kept_compiled_target_loads_with_its_hash() {
    let root = contracts();
    let Ok(entries) = std::fs::read_dir(&root) else {
        // No release has been kept yet.
        return;
    };
    let mut problems = Vec::new();
    for entry in entries.flatten() {
        let folder = entry.path();
        if folder.is_dir() {
            problems.extend(problems_of(&folder));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_kept_folder_with_no_compiled_target_is_a_problem() {
    let folder = std::env::temp_dir().join(format!("a2m-kept-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    assert_eq!(problems_of(&folder).len(), 1);
    std::fs::write(folder.join("compiled-targets.json"), "[]").unwrap();
    assert_eq!(problems_of(&folder).len(), 1);
    std::fs::write(
        folder.join("compiled-targets.json"),
        r#"[{"file": "compiled/example.format-1.target", "format": 1, "hash": "00"}]"#,
    )
    .unwrap();
    std::fs::create_dir_all(folder.join("compiled")).unwrap();
    std::fs::write(
        folder.join("compiled/example.format-1.target"),
        b"anything2mcp-plan 1\n{}",
    )
    .unwrap();
    let found = problems_of(&folder);
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("is not a hash"), "{found:?}");
}
