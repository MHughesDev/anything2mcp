//! The compiled target of the `example` target, as a release keeps it
//! (decision 0437, point 1): compiled from `targets/example/` as published,
//! in the compiled target format this build writes, with its hash.
//!
//! The test always compiles the target and proves that the bytes decode with
//! their hash and encode back to the same bytes, so that what a release keeps
//! is what the engine reads. When `A2M_CONTRACT_COMPILED_OUT` names a folder,
//! it also writes the bytes there as `example.format-<N>.target`, for
//! `tools/contracts.py write`; nothing else sets that variable.

use a2m_core::{COMPILED_TARGET_FORMAT, CompiledTarget, Version};
use a2m_files::testing::{example_root, stand_set};
use a2m_files::{CompileMode, compile, load};

/// The version a kept compiled target is published as: fixed, so that the
/// same sources give the same bytes in every release's folder.
const VERSION: Version = Version::new(1, 0, 0);

/// The variable that names the folder to write the bytes into.
const OUT: &str = "A2M_CONTRACT_COMPILED_OUT";

#[test]
fn the_example_target_compiles_to_bytes_that_decode_with_their_hash_and_encode_back_the_same() {
    let set = stand_set().unwrap();
    let sources = load(&example_root(), &set).unwrap_or_else(|problems| panic!("{problems}"));
    let compiled = compile(&sources, &set, CompileMode::Publish, VERSION)
        .unwrap_or_else(|problems| panic!("{problems}"));
    let bytes = &compiled.encoded.bytes;
    let decoded = CompiledTarget::decode(bytes, &compiled.encoded.hash)
        .unwrap_or_else(|failure| panic!("{failure:?}"));
    assert_eq!(decoded.header.format, COMPILED_TARGET_FORMAT);
    let again = decoded
        .encode()
        .unwrap_or_else(|failure| panic!("{failure:?}"));
    assert_eq!(
        &again.bytes, bytes,
        "decoding and encoding changed the bytes"
    );
    assert_eq!(again.hash, compiled.encoded.hash);

    // The same sources give the same bytes: a kept compiled target can be
    // written again and compared.
    let twice = compile(&sources, &set, CompileMode::Publish, VERSION)
        .unwrap_or_else(|problems| panic!("{problems}"));
    assert_eq!(&twice.encoded.bytes, bytes);

    if let Some(folder) = std::env::var_os(OUT) {
        let folder = std::path::PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(format!("example.format-{COMPILED_TARGET_FORMAT}.target"));
        std::fs::write(&file, bytes).unwrap();
        println!("wrote {} ({})", file.display(), compiled.encoded.hash);
    }
}
