//! The plain facts of a binary (X5, decision 0276): what the scout reads
//! from the bytes of an ELF, a PE, and a Mach-O file it is handed, and that
//! it starts nothing to do so. The program of the test stand is read as the
//! system's own tools read it. A PE file and a Mach-O file are built here,
//! byte by byte, because only one of the three exists on each system; the
//! values they state were also read from the files by `llvm-readobj`
//! (write them out with `A2M_BINARY_FIXTURES_DIR` set).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2m_core::{
    AccessKind, Coverage, Environment, EvidenceLevel, Failure, Finding, LimitationCode, NoSecrets,
    Redact, Step, SurfaceKind, Values,
};
use a2m_pack_command::binary::BinaryFound;
use a2m_pack_command::codes;
use a2m_provider::{
    Access, BoxFuture, CheckedPaths, Deadline, DiscoveryReport, DiscoveryScope, Registry, RootName,
    Roots, SandboxPort, StepFailure, StepInput, StepOutcome, SurfaceDescription, SurfaceRef,
};
use common::{Folder, registry};
use example_cli::harness::Stand;

const WITHIN: Duration = Duration::from_secs(60);

// ---- A port that fails the test when anything is started. ----

#[derive(Default)]
struct StartsNothing(AtomicUsize);

impl SandboxPort for StartsNothing {
    fn act<'a>(
        &'a self,
        _step: &'a Step,
        _input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Err(StepFailure::not_sent(codes::row::STEP_NOT_USABLE.failure(
                "A binary reader asked for a step.",
                "It starts nothing.",
            )))
        })
    }
}

struct Found(Vec<Finding>);

impl a2m_provider::FindingSink for Found {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// A redactor that knows one planted secret.
struct Planted(&'static str);

impl Redact for Planted {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, a2m_core::REDACTED)
    }
}

struct Scene {
    folder: Folder,
    registry: Registry,
    surface: SurfaceRef,
    _stand: Stand,
}

fn scene(redact: Arc<dyn Redact>) -> Scene {
    let folder = Folder::new("binary");
    let stand = Stand::start().unwrap();
    let registry = registry(&folder, redact);
    let compiled_target = common::stand_compiled_target(&stand, registry.compiler_set());
    let surface = SurfaceRef {
        target: compiled_target.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface: compiled_target.surfaces[0].clone(),
    };
    Scene {
        folder,
        registry,
        surface,
        _stand: stand,
    }
}

fn description(surface: &SurfaceRef) -> SurfaceDescription {
    SurfaceDescription {
        kind: SurfaceKind::new("program").unwrap(),
        access_kind: AccessKind::Command,
        address: surface.surface.address.clone(),
        facts: Values::new(),
    }
}

/// Hands `bytes` to the pack's binary discoverer under `binary`, and
/// reads them. Nothing may be started.
async fn read(
    scene: &Scene,
    bytes: &[u8],
    redact: &dyn Redact,
) -> Result<(DiscoveryReport, Vec<Finding>), Failure> {
    let sources = scene.folder.path().join("inputs");
    std::fs::create_dir_all(&sources).unwrap();
    std::fs::write(sources.join("program.bin"), bytes).unwrap();
    let root = RootName::new("inputs").unwrap();
    let mut roots = Roots::none();
    roots.add(root.clone(), &sources, Access::Read).unwrap();
    let mut paths = CheckedPaths::none();
    paths.insert(
        "binary",
        roots.check(&root, "program.bin", Access::Read).unwrap(),
    );
    let discoverers: Vec<_> = scene
        .registry
        .discoverers_for(&description(&scene.surface))
        .into_iter()
        .filter(|found| found.declared().name.to_string() == "binary")
        .collect();
    assert_eq!(discoverers.len(), 1);
    let port = StartsNothing::default();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverers[0].declared().bounds,
        deadline: Deadline::after(WITHIN),
        paths: &paths,
        redact,
        focus: &Values::new(),
    };
    let mut found = Found(Vec::new());
    let report = discoverers[0].discover(&port, &scope, &mut found).await;
    assert_eq!(
        port.0.load(Ordering::SeqCst),
        0,
        "a binary reader started something"
    );
    report.map(|report| (report, found.0))
}

async fn facts(scene: &Scene, bytes: &[u8]) -> BinaryFound {
    let (report, findings) = read(scene, bytes, &NoSecrets).await.unwrap();
    assert!(report.bounds_hit.is_empty(), "{report:?}");
    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.level(), EvidenceLevel::Inferred);
    assert_eq!(finding.kind().as_str(), "binary");
    serde_json::from_str(finding.found()).unwrap()
}

/// The most imports or exports one finding lists (`MOST_LISTED` of the
/// pack); a program with more is reported as read in part, by name.
const MOST_LISTED: usize = 2_000;

/// Like `facts`, for a program of the system's own build, which may hold more
/// imports or exports than a finding lists (a Mach-O file lists a bind for
/// each place that uses a symbol). The pack's bound is not loosened and the
/// program is not skipped: when the lists are cut, the test requires that
/// the pack said so (`bytes` as the bound hit, partial coverage, a
/// truncation limitation), that a list is full, and that the totals count
/// what the lists could not hold. Answers whether they were cut.
async fn facts_of_a_large_program(scene: &Scene, bytes: &[u8]) -> (BinaryFound, bool) {
    let (report, findings) = read(scene, bytes, &NoSecrets).await.unwrap();
    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.level(), EvidenceLevel::Inferred);
    assert_eq!(finding.kind().as_str(), "binary");
    let found: BinaryFound = serde_json::from_str(finding.found()).unwrap();
    let cut = found.imports_total as usize > found.imports.len()
        || found.exports_total as usize > found.exports.len();
    if cut {
        assert_eq!(report.bounds_hit, ["listed"], "{report:?}");
        assert_eq!(finding.coverage(), Coverage::Partial);
        assert!(
            finding
                .limitations()
                .iter()
                .any(|limit| limit.code == LimitationCode::Truncated),
            "{:?}",
            finding.limitations()
        );
        assert!(
            found.imports.len() == MOST_LISTED || found.exports.len() == MOST_LISTED,
            "{} imports and {} exports are listed",
            found.imports.len(),
            found.exports.len()
        );
    } else {
        assert!(report.bounds_hit.is_empty(), "{report:?}");
    }
    (found, cut)
}

async fn stop(scene: &Scene, bytes: &[u8]) -> Failure {
    match read(scene, bytes, &NoSecrets).await {
        Ok(_) => panic!("the file was read"),
        Err(failure) => failure,
    }
}

// ---- Putting bytes together. ----

fn put(out: &mut Vec<u8>, at: usize, bytes: &[u8]) {
    if out.len() < at + bytes.len() {
        out.resize(at + bytes.len(), 0);
    }
    out[at..at + bytes.len()].copy_from_slice(bytes);
}

fn u16le(out: &mut Vec<u8>, at: usize, value: u16) {
    put(out, at, &value.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, at: usize, value: u32) {
    put(out, at, &value.to_le_bytes());
}

fn u32be(out: &mut Vec<u8>, at: usize, value: u32) {
    put(out, at, &value.to_be_bytes());
}

fn u64le(out: &mut Vec<u8>, at: usize, value: u64) {
    put(out, at, &value.to_le_bytes());
}

fn utf16z(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn pad4(bytes: &mut Vec<u8>) {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
}

/// One block of a version resource: length, value length, type, key, value,
/// then children, each part on a four-byte boundary.
fn version_block(key: &str, value: &[u8], text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0; 6];
    out.extend(utf16z(key));
    pad4(&mut out);
    out.extend_from_slice(value);
    pad4(&mut out);
    for child in children {
        out.extend_from_slice(child);
        pad4(&mut out);
    }
    let length = u16::try_from(out.len()).unwrap();
    let value_length = if text { value.len() / 2 } else { value.len() };
    u16le(&mut out, 0, length);
    u16le(&mut out, 2, u16::try_from(value_length).unwrap());
    u16le(&mut out, 4, u16::from(text));
    out
}

fn version_resource() -> Vec<u8> {
    let mut fixed = vec![0; 52];
    u32le(&mut fixed, 0, 0xfeef_04bd);
    u32le(&mut fixed, 4, 0x0001_0000);
    u32le(&mut fixed, 8, (3 << 16) | 2); // file version 3.2.
    u32le(&mut fixed, 12, (1 << 16) | 7); // .1.7
    u32le(&mut fixed, 16, (3 << 16) | 2);
    u32le(&mut fixed, 20, (1 << 16) | 7);
    let string = |name: &str, value: &str| version_block(name, &utf16z(value), true, &[]);
    let table = version_block(
        "040904b0",
        &[],
        true,
        &[
            string("CompanyName", "Example Company"),
            string("FileVersion", "3.2.1.7"),
            string("ProductName", "Example Program"),
        ],
    );
    let info = version_block("StringFileInfo", &[], true, &[table]);
    let translation = version_block("Translation", &[0x09, 0x04, 0xb0, 0x04], false, &[]);
    let var = version_block("VarFileInfo", &[], true, &[translation]);
    version_block("VS_VERSION_INFO", &fixed, false, &[info, var])
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="Example.Program" version="3.2.1.7"/>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges>
      <requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>
    </requestedPrivileges></security>
  </trustInfo>
</assembly>"#;

/// A PE32+ console program of 64-bit x86: an import table (two imports of
/// one library), an export table (two exports), a resource tree (a version
/// resource and a manifest), and a certificate table.
fn pe_file() -> Vec<u8> {
    let mut f = vec![0_u8; 0xa20];
    put(&mut f, 0, b"MZ");
    u32le(&mut f, 0x3c, 0x40);
    put(&mut f, 0x40, b"PE\0\0");
    u16le(&mut f, 0x44, 0x8664); // machine
    u16le(&mut f, 0x46, 3); // sections
    u16le(&mut f, 0x54, 0xf0); // size of optional header
    u16le(&mut f, 0x56, 0x0022); // executable, large addresses
    let opt = 0x58;
    u16le(&mut f, opt, 0x20b);
    u32le(&mut f, opt + 32, 0x1000); // section alignment
    u32le(&mut f, opt + 36, 0x200); // file alignment
    u32le(&mut f, opt + 56, 0x4000); // size of image
    u32le(&mut f, opt + 60, 0x200); // size of headers
    u16le(&mut f, opt + 68, 3); // console
    u32le(&mut f, opt + 108, 16);
    let dirs = opt + 112;
    u32le(&mut f, dirs, 0x3000); // export
    u32le(&mut f, dirs + 4, 0x200);
    u32le(&mut f, dirs + 8, 0x1000); // import
    u32le(&mut f, dirs + 12, 40);
    u32le(&mut f, dirs + 16, 0x2000); // resource
    u32le(&mut f, dirs + 20, 0x400);
    u32le(&mut f, dirs + 32, 0xa00); // certificate: a file offset
    u32le(&mut f, dirs + 36, 0x20);
    let table = opt + 0xf0;
    let section = |f: &mut Vec<u8>, index: usize, name: &[u8], rva: u32, raw: u32, size: u32| {
        let at = table + index * 40;
        put(f, at, name);
        u32le(f, at + 8, size);
        u32le(f, at + 12, rva);
        u32le(f, at + 16, size);
        u32le(f, at + 20, raw);
        u32le(f, at + 36, 0x4000_0040);
    };
    section(&mut f, 0, b".idata", 0x1000, 0x200, 0x200);
    section(&mut f, 1, b".rsrc", 0x2000, 0x400, 0x400);
    section(&mut f, 2, b".edata", 0x3000, 0x800, 0x200);

    // .idata
    let idata = 0x200;
    u32le(&mut f, idata, 0x1040); // original first thunk
    u32le(&mut f, idata + 12, 0x1080); // name
    u32le(&mut f, idata + 16, 0x1060); // first thunk
    for at in [0x40, 0x60] {
        u64le(&mut f, idata + at, 0x1090);
        u64le(&mut f, idata + at + 8, 0x10a0);
    }
    put(&mut f, idata + 0x80, b"KERNEL32.dll\0");
    put(&mut f, idata + 0x92, b"ExitProcess\0");
    put(&mut f, idata + 0xa2, b"GetLastError\0");

    // .rsrc: root -> type -> name -> language -> data.
    let rsrc = 0x400;
    let directory = |f: &mut Vec<u8>, at: usize, id: u32, to: u32, is_table: bool| {
        u16le(f, rsrc + at + 14, 1); // one entry with an id
        u32le(f, rsrc + at + 16, id);
        u32le(
            f,
            rsrc + at + 20,
            if is_table { 0x8000_0000 | to } else { to },
        );
    };
    u16le(&mut f, rsrc + 14, 2); // the root has two
    u32le(&mut f, rsrc + 16, 16); // RT_VERSION
    u32le(&mut f, rsrc + 20, 0x8000_0020);
    u32le(&mut f, rsrc + 24, 24); // RT_MANIFEST
    u32le(&mut f, rsrc + 28, 0x8000_0060);
    directory(&mut f, 0x20, 1, 0x38, true);
    directory(&mut f, 0x38, 0x409, 0x50, false);
    directory(&mut f, 0x60, 1, 0x78, true);
    directory(&mut f, 0x78, 0x409, 0x90, false);
    let version = version_resource();
    u32le(&mut f, rsrc + 0x50, 0x2000 + 0xa0);
    u32le(&mut f, rsrc + 0x54, u32::try_from(version.len()).unwrap());
    put(&mut f, rsrc + 0xa0, &version);
    u32le(&mut f, rsrc + 0x90, 0x2000 + 0x200);
    u32le(&mut f, rsrc + 0x94, u32::try_from(MANIFEST.len()).unwrap());
    put(&mut f, rsrc + 0x200, MANIFEST.as_bytes());

    // .edata
    let edata = 0x800;
    u32le(&mut f, edata + 12, 0x3040); // name
    u32le(&mut f, edata + 16, 1); // base
    u32le(&mut f, edata + 20, 2);
    u32le(&mut f, edata + 24, 2);
    u32le(&mut f, edata + 28, 0x3028);
    u32le(&mut f, edata + 32, 0x3030);
    u32le(&mut f, edata + 36, 0x3038);
    u32le(&mut f, edata + 0x28, 0x1000);
    u32le(&mut f, edata + 0x2c, 0x1000);
    u32le(&mut f, edata + 0x30, 0x3060);
    u32le(&mut f, edata + 0x34, 0x3070);
    u16le(&mut f, edata + 0x38, 0);
    u16le(&mut f, edata + 0x3a, 1);
    put(&mut f, edata + 0x40, b"example.exe\0");
    put(&mut f, edata + 0x60, b"ExportedOne\0");
    put(&mut f, edata + 0x70, b"ExportedTwo\0");

    // The certificate table: one WIN_CERTIFICATE of 0x20 bytes.
    u32le(&mut f, 0xa00, 0x20);
    u16le(&mut f, 0xa04, 0x0200);
    u16le(&mut f, 0xa06, 0x0002);
    f
}

const CODE_DIRECTORY: u32 = 0xfade_0c02;
const ENTITLEMENTS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>com.apple.security.app-sandbox</key><true/><key>com.apple.security.network.client</key><true/><key>com.example.groups</key><array><string>group.one</string></array></dict></plist>";

fn code_signature() -> Vec<u8> {
    let identifier = b"com.example.tool\0";
    let team = b"EXAMPLE001\0";
    let mut directory = vec![0_u8; 52];
    u32be(&mut directory, 0, CODE_DIRECTORY);
    u32be(&mut directory, 8, 0x0002_0200); // version: has a team identifier
    u32be(&mut directory, 12, 0); // flags: not ad hoc
    u32be(&mut directory, 16, 52 + 17 + 11); // hash offset
    u32be(&mut directory, 20, 52); // identifier offset
    directory[36] = 32; // hash size
    directory[37] = 2; // SHA-256
    directory[39] = 12;
    u32be(&mut directory, 48, 52 + 17); // team offset
    directory.extend_from_slice(identifier);
    directory.extend_from_slice(team);
    let length = u32::try_from(directory.len()).unwrap();
    u32be(&mut directory, 4, length);

    let mut entitlements = vec![0_u8; 8];
    u32be(&mut entitlements, 0, 0xfade_7171);
    entitlements.extend_from_slice(ENTITLEMENTS.as_bytes());
    let length = u32::try_from(entitlements.len()).unwrap();
    u32be(&mut entitlements, 4, length);

    let first = 12 + 16;
    let second = first + directory.len();
    let mut blob = vec![0_u8; 12];
    u32be(&mut blob, 0, 0xfade_0cc0);
    u32be(&mut blob, 8, 2);
    blob.extend_from_slice(&0_u32.to_be_bytes()); // slot: code directory
    blob.extend_from_slice(&u32::try_from(first).unwrap().to_be_bytes());
    blob.extend_from_slice(&5_u32.to_be_bytes()); // slot: entitlements
    blob.extend_from_slice(&u32::try_from(second).unwrap().to_be_bytes());
    blob.extend_from_slice(&directory);
    blob.extend_from_slice(&entitlements);
    let length = u32::try_from(blob.len()).unwrap();
    u32be(&mut blob, 4, length);
    blob
}

fn command(out: &mut Vec<u8>, cmd: u32, body: &[u8]) {
    let size = (8 + body.len()).next_multiple_of(8);
    out.extend_from_slice(&cmd.to_le_bytes());
    out.extend_from_slice(&u32::try_from(size).unwrap().to_le_bytes());
    out.extend_from_slice(body);
    out.resize(out.len() + (size - 8 - body.len()), 0);
}

/// A 64-bit ARM Mach-O program for macOS: one library, one import by bind
/// opcodes, one export, a build version, and a signature with a team
/// identifier and entitlements.
fn macho_file() -> Vec<u8> {
    // The data the commands point at.
    let mut bind = vec![0x11, 0x40];
    bind.extend_from_slice(b"_example_import\0");
    bind.extend_from_slice(&[0x51, 0x72, 0x00, 0x90, 0x00]);
    let mut exports = vec![0x00, 0x01];
    exports.extend_from_slice(b"_main\0");
    exports.push(9);
    exports.extend_from_slice(&[0x02, 0x00, 0x10, 0x00]);
    let signature = code_signature();

    let mut commands = Vec::new();
    // The one segment covers the whole file; its length is put in later.
    let mut segment = [0_u8; 64];
    segment[..6].copy_from_slice(b"__TEXT");
    command(&mut commands, 0x19, &segment);
    let dyld_at = commands.len();
    command(&mut commands, 0x8000_0022, &[0; 40]);
    let mut dylib = vec![0_u8; 16];
    dylib[..4].copy_from_slice(&24_u32.to_le_bytes());
    dylib.extend_from_slice(b"/usr/lib/libSystem.B.dylib\0");
    command(&mut commands, 0xc, &dylib);
    let mut build = Vec::new();
    build.extend_from_slice(&1_u32.to_le_bytes()); // macOS
    build.extend_from_slice(&0x000e_0000_u32.to_le_bytes());
    build.extend_from_slice(&0x000e_0000_u32.to_le_bytes());
    build.extend_from_slice(&0_u32.to_le_bytes());
    command(&mut commands, 0x32, &build);
    let signature_at = commands.len();
    command(&mut commands, 0x1d, &[0; 8]);

    let header = 32;
    let data = header + commands.len();
    let bind_at = data;
    let exports_at = (bind_at + bind.len()).next_multiple_of(8);
    let signature_start = (exports_at + exports.len()).next_multiple_of(16);
    let total = signature_start + signature.len();

    // Fill in what points at what.
    let seg = 8; // the body of the segment command
    commands[seg + 16 + 16..seg + 16 + 24].copy_from_slice(&0_u64.to_le_bytes());
    commands[seg + 16 + 24..seg + 16 + 32].copy_from_slice(&(total as u64).to_le_bytes());
    let dyld = dyld_at + 8;
    let word = |commands: &mut Vec<u8>, at: usize, value: usize| {
        commands[at..at + 4].copy_from_slice(&u32::try_from(value).unwrap().to_le_bytes());
    };
    word(&mut commands, dyld + 16, bind_at);
    word(&mut commands, dyld + 20, bind.len());
    word(&mut commands, dyld + 32, exports_at);
    word(&mut commands, dyld + 36, exports.len());
    word(&mut commands, signature_at + 8, signature_start);
    word(&mut commands, signature_at + 12, signature.len());

    let mut f = vec![0_u8; header];
    f[..4].copy_from_slice(&0xfeed_facf_u32.to_le_bytes());
    f[4..8].copy_from_slice(&0x0100_000c_u32.to_le_bytes()); // arm64
    f[12..16].copy_from_slice(&2_u32.to_le_bytes()); // executable
    f[16..20].copy_from_slice(&5_u32.to_le_bytes());
    f[20..24].copy_from_slice(&u32::try_from(commands.len()).unwrap().to_le_bytes());
    f.extend_from_slice(&commands);
    put(&mut f, bind_at, &bind);
    put(&mut f, exports_at, &exports);
    put(&mut f, signature_start, &signature);
    f
}

/// A fat file of one slice at `offset`, which lists `arch_offset` for it.
fn fat_file(slice: &[u8], offset: usize, arch_offset: usize) -> Vec<u8> {
    let mut f = vec![0_u8; offset];
    u32be(&mut f, 0, 0xcafe_babe);
    u32be(&mut f, 4, 1);
    u32be(&mut f, 8, 0x0100_000c);
    u32be(&mut f, 16, u32::try_from(arch_offset).unwrap());
    u32be(&mut f, 20, u32::try_from(slice.len()).unwrap());
    u32be(&mut f, 24, 12);
    f.extend_from_slice(slice);
    f
}

fn write_out(name: &str, bytes: &[u8]) {
    if let Some(dir) = std::env::var_os("A2M_BINARY_FIXTURES_DIR") {
        std::fs::write(std::path::Path::new(&dir).join(name), bytes).unwrap();
    }
}

// ---- The tests. ----

#[tokio::test(flavor = "multi_thread")]
async fn a_pe_file_gives_its_plain_facts_and_starts_nothing() {
    let scene = scene(Arc::new(NoSecrets));
    let file = pe_file();
    write_out("fixture.exe", &file);
    let found = facts(&scene, &file).await;
    assert_eq!(found.format, "pe");
    assert_eq!(found.system, "windows");
    assert_eq!(found.architecture, "x86_64");
    assert_eq!((found.bits, found.endian.as_str()), (64, "little"));
    assert_eq!(found.kind, "executable");
    assert_eq!(found.subsystem.as_deref(), Some("console"));
    assert_eq!(found.libraries, ["KERNEL32.dll"]);
    let imports: Vec<_> = found
        .imports
        .iter()
        .map(|import| (import.library.as_str(), import.name.as_str()))
        .collect();
    assert_eq!(
        imports,
        [
            ("KERNEL32.dll", "ExitProcess"),
            ("KERNEL32.dll", "GetLastError")
        ]
    );
    assert_eq!(found.exports, ["ExportedOne", "ExportedTwo"]);
    assert_eq!((found.imports_total, found.exports_total), (2, 2));
    let version = found.version.unwrap();
    assert_eq!(version.file_version.as_deref(), Some("3.2.1.7"));
    assert_eq!(version.product_version.as_deref(), Some("3.2.1.7"));
    assert_eq!(version.strings["ProductName"], "Example Program");
    assert_eq!(version.strings["CompanyName"], "Example Company");
    let manifest = found.manifest.unwrap();
    assert_eq!(
        manifest.identity.as_deref(),
        Some("Example.Program 3.2.1.7")
    );
    assert_eq!(
        manifest.execution_level.as_deref(),
        Some("requireAdministrator")
    );
    assert_eq!(manifest.ui_access, Some(false));
    // Who signed it was not read: only that a certificate table is there.
    let signer = found.signer.unwrap();
    assert_eq!((signer.form.as_str(), signer.bytes), ("authenticode", 0x20));
    assert_eq!(signer.team, None);
    assert!(found.unread.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mach_o_file_gives_its_plain_facts_entitlements_and_signer() {
    let scene = scene(Arc::new(NoSecrets));
    let file = macho_file();
    write_out("fixture.macho", &file);
    let found = facts(&scene, &file).await;
    assert_eq!(found.format, "macho");
    assert_eq!(found.system, "macos");
    assert_eq!(found.architecture, "aarch64");
    assert_eq!((found.bits, found.endian.as_str()), (64, "little"));
    assert_eq!(found.kind, "executable");
    assert_eq!(found.libraries, ["/usr/lib/libSystem.B.dylib"]);
    assert_eq!(found.imports.len(), 1);
    assert_eq!(found.imports[0].name, "_example_import");
    assert_eq!(found.imports[0].library, "/usr/lib/libSystem.B.dylib");
    assert_eq!(found.exports, ["_main"]);
    assert_eq!(
        found.entitlements,
        [
            "com.apple.security.app-sandbox",
            "com.apple.security.network.client",
            "com.example.groups"
        ]
    );
    let signer = found.signer.unwrap();
    assert_eq!(signer.form, "code_signature");
    assert_eq!(signer.identifier.as_deref(), Some("com.example.tool"));
    assert_eq!(signer.team.as_deref(), Some("EXAMPLE001"));
    assert_eq!(signer.ad_hoc, Some(false));
    assert!(found.unread.is_empty(), "{:?}", found.unread);

    // The same program in a fat file: each slice is read.
    let fat = fat_file(&file, 0x1000, 0x1000);
    let found = facts(&scene, &fat).await;
    assert_eq!(found.format, "macho_fat");
    assert_eq!(found.slices.len(), 1);
    assert_eq!(
        found.slices[0].signer.as_ref().unwrap().team.as_deref(),
        Some("EXAMPLE001")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_text_the_file_holds_passes_the_redactor_first() {
    let secret = "Example Program";
    let scene = scene(Arc::new(Planted(secret)));
    let (_, findings) = read(&scene, &pe_file(), &Planted(secret)).await.unwrap();
    let text = findings[0].found();
    assert!(!text.contains(secret), "{text}");
    assert!(text.contains(a2m_core::REDACTED), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_cut_short_is_a_named_stop_wherever_it_is_cut() {
    let scene = scene(Arc::new(NoSecrets));
    let truncated = |failure: &Failure| failure.code() == &codes::BINARY_TRUNCATED;
    // Each part of a PE file: the signature, the section table, the data a
    // section holds, and the certificate table.
    let pe = pe_file();
    for cut in [2, 0x30, 0x100, 0x1c0, 0x500, 0x9ff, 0xa10] {
        let failure = stop(&scene, &pe[..cut]).await;
        assert!(truncated(&failure), "pe cut at {cut}: {failure:?}");
    }
    // The load commands, the segment, and the signature of a Mach-O file.
    let macho = macho_file();
    for cut in [3, 20, 60, macho.len() - 5, macho.len() / 2] {
        let failure = stop(&scene, &macho[..cut]).await;
        assert!(truncated(&failure), "macho cut at {cut}: {failure:?}");
    }
    // A fat file whose slice runs past the end.
    let fat = fat_file(&macho, 0x1000, 0x1000);
    let failure = stop(&scene, &fat[..fat.len() - 1]).await;
    assert!(truncated(&failure), "{failure:?}");
    // And an empty one.
    assert!(truncated(&stop(&scene, b"").await));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_that_leads_back_to_itself_is_a_named_stop() {
    let scene = scene(Arc::new(NoSecrets));
    let cyclic = |failure: &Failure| failure.code() == &codes::BINARY_CYCLIC;
    // A language table of the resource tree whose entry is the root.
    let mut pe = pe_file();
    u32le(&mut pe, 0x400 + 0x38 + 20, 0x8000_0000);
    let failure = stop(&scene, &pe).await;
    assert!(cyclic(&failure), "{failure:?}");
    // A name table whose entry is the table itself.
    let mut pe = pe_file();
    u32le(&mut pe, 0x400 + 0x20 + 20, 0x8000_0020);
    let failure = stop(&scene, &pe).await;
    assert!(cyclic(&failure), "{failure:?}");
    // A slice of a fat file that is the fat header, and one that is a fat
    // file.
    let macho = macho_file();
    let failure = stop(&scene, &fat_file(&macho, 0x1000, 0)).await;
    assert!(cyclic(&failure), "{failure:?}");
    let inner = fat_file(&macho, 0x1000, 0x1000);
    let failure = stop(&scene, &fat_file(&inner, 0x1000, 0x1000)).await;
    assert!(cyclic(&failure), "{failure:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_of_no_known_kind_or_not_well_formed_is_a_named_stop() {
    let scene = scene(Arc::new(NoSecrets));
    let failure = stop(&scene, b"#!/bin/sh\necho hello\n").await;
    assert_eq!(failure.code(), &codes::BINARY_KIND_UNKNOWN);
    // An MZ file with no PE signature is an older format.
    let mut dos = pe_file();
    put(&mut dos, 0x40, b"NE\0\0");
    let failure = stop(&scene, &dos).await;
    assert_eq!(failure.code(), &codes::BINARY_KIND_UNKNOWN);
    // A resource tree deeper than three levels.
    let mut deep = pe_file();
    u32le(&mut deep, 0x400 + 0x38 + 20, 0x8000_0060);
    let failure = stop(&scene, &deep).await;
    assert_eq!(failure.code(), &codes::BINARY_MALFORMED, "{failure:?}");
    // A load command smaller than its own header.
    let mut macho = macho_file();
    u32le(&mut macho, 32 + 4, 4);
    let failure = stop(&scene, &macho).await;
    assert_eq!(failure.code(), &codes::BINARY_MALFORMED, "{failure:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_handed_then_nothing_is_read() {
    let scene = scene(Arc::new(NoSecrets));
    let discoverers: Vec<_> = scene
        .registry
        .discoverers_for(&description(&scene.surface))
        .into_iter()
        .filter(|found| found.declared().name.to_string() == "binary")
        .collect();
    let port = StartsNothing::default();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverers[0].declared().bounds,
        deadline: Deadline::after(WITHIN),
        paths: &CheckedPaths::none(),
        redact: &NoSecrets,
        focus: &Values::new(),
    };
    let mut found = Found(Vec::new());
    let report = discoverers[0]
        .discover(&port, &scope, &mut found)
        .await
        .unwrap();
    assert_eq!(report, DiscoveryReport::default());
    assert!(found.0.is_empty());
}

// ---- The program of the test stand, against the system's own tools. ----

fn tool(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stand_program_agrees_with_the_system_tools() {
    let scene = scene(Arc::new(NoSecrets));
    let stand = Stand::start().unwrap();
    let program = stand.program().to_path_buf();
    let bytes = std::fs::read(&program).unwrap();
    let (found, cut) = facts_of_a_large_program(&scene, &bytes).await;
    assert_eq!(found.kind, "executable");
    assert_eq!(found.bits, 64);
    assert_eq!(found.architecture, std::env::consts::ARCH);
    let path = program.to_str().unwrap();

    if cfg!(target_os = "linux") {
        assert_eq!(
            (found.format.as_str(), found.system.as_str()),
            ("elf", "unix_sysv")
        );
        let header = tool("readelf", &["-h", path]).expect("readelf is on a Linux machine");
        assert!(header.contains("ELF64"), "{header}");
        assert!(header.contains("UNIX - System V"), "{header}");
        assert!(header.contains("Position-Independent Executable") || header.contains("EXEC"));
        let dynamic = tool("readelf", &["-d", "-W", path]).unwrap();
        let needed: BTreeSet<String> = dynamic
            .lines()
            .filter(|line| line.contains("(NEEDED)"))
            .filter_map(|line| line.split('[').nth(1)?.split(']').next())
            .map(str::to_owned)
            .collect();
        let ours: BTreeSet<String> = found.libraries.iter().cloned().collect();
        assert_eq!(ours, needed);
        // The dynamic symbols: those that are undefined are imports, and
        // those that are defined are exports.
        let symbols = tool("readelf", &["--dyn-syms", "-W", path]).unwrap();
        let (mut imports, mut exports) = (BTreeSet::new(), BTreeSet::new());
        for line in symbols.lines().skip(3) {
            let columns: Vec<&str> = line.split_whitespace().collect();
            if columns.len() < 8 {
                continue;
            }
            let name = columns[7].split('@').next().unwrap().to_owned();
            if columns[6] == "UND" {
                imports.insert(name);
            } else {
                exports.insert(name);
            }
        }
        let our_imports: BTreeSet<String> = found
            .imports
            .iter()
            .map(|import| import.name.clone())
            .collect();
        let our_exports: BTreeSet<String> = found.exports.iter().cloned().collect();
        if !cut {
            assert_eq!(our_imports, imports);
            assert_eq!(our_exports, exports);
            assert_eq!(found.imports_total as usize, found.imports.len());
        }
        assert!(found.signer.is_none());
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            (found.format.as_str(), found.system.as_str()),
            ("macho", "macos")
        );
        let libraries = tool("otool", &["-L", path]).expect("otool is on a macOS machine");
        let listed: BTreeSet<String> = libraries
            .lines()
            .skip(1)
            .filter_map(|line| line.trim().split(" (").next())
            .map(str::to_owned)
            .collect();
        let ours: BTreeSet<String> = found.libraries.iter().cloned().collect();
        assert_eq!(ours, listed);
        // Every symbol the system's tool lists as undefined is an import.
        if let Some(undefined) = tool("nm", &["-u", path]).filter(|_| !cut) {
            let ours: BTreeSet<String> = found
                .imports
                .iter()
                .map(|import| import.name.clone())
                .collect();
            for line in undefined.lines().map(str::trim).filter(|l| !l.is_empty()) {
                assert!(ours.contains(line), "`{line}` is not among {ours:?}");
            }
        }
    } else {
        // Where no tool is a given, what any PE program holds is checked.
        assert_eq!(
            (found.format.as_str(), found.system.as_str()),
            ("pe", "windows")
        );
        assert!(
            found
                .libraries
                .iter()
                .any(|library| library.eq_ignore_ascii_case("kernel32.dll")),
            "{:?}",
            found.libraries
        );
        assert!(found.subsystem.is_some());
    }
}
