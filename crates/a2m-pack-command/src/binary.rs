//! The plain facts of a binary: what kind of file it is and for which
//! system, what it imports and exports, its version resource and manifest,
//! its entitlements, and who signed it (X5, decision 0276).
//!
//! The file is read as bytes. It is never run, loaded, or handed to the
//! system's own loader, and nothing it holds is evaluated. The headers are
//! read by `object` (Apache-2.0 OR MIT, the readers of ELF, PE, and
//! Mach-O only); the extent a header declares, the version resource, the
//! manifest, and the entitlements are read here, because they are small
//! and the library reads none of them.
//!
//! Every read is bounded: a list is cut at [`MOST_LISTED`] and says so, a
//! text at [`MOST_TEXT`] characters after it was redacted whole, a resource
//! tree at three tables deep, a fat file at [`MOST_SLICES`] slices. A file
//! that is shorter than its own headers say, one that leads back to
//! itself, one of a kind with no rule, and one that is damaged are four
//! named stops, never a guess.

use std::collections::BTreeMap;

use a2m_core::Redact;
use object::read::macho::CodeSignature;
use object::read::pe::{PeFile32, PeFile64};
use object::{Object, ObjectKind, macho};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

/// The most imports, and the most exports, one finding lists.
pub(crate) const MOST_LISTED: usize = 2_000;
/// The most characters of any one text in a finding.
const MOST_TEXT: usize = 256;
/// The most slices of a fat Mach-O file that are read.
const MOST_SLICES: usize = 16;
/// The most entries of a resource tree that are looked at.
const MOST_RESOURCE_ENTRIES: usize = 4_096;
/// The most bytes of one resource that are read.
const MOST_RESOURCE_BYTES: usize = 1 << 20;
/// The most strings of a version resource.
const MOST_VERSION_STRINGS: usize = 64;
/// The most entitlement keys.
const MOST_KEYS: usize = 256;
/// The most events of a manifest or an entitlement document.
const MOST_EVENTS: usize = 10_000;

/// A file this reader stops at, and why. Each is one failure code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The file is of no kind this reader has a rule for.
    Kind(String),
    /// The file is shorter than its own headers say.
    Truncated(String),
    /// The file leads back to a part of itself it is reading.
    Cyclic(String),
    /// The file is none of the above and is not well formed.
    Malformed(String),
}

/// One imported symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportFound {
    /// The library it comes from, where the format says (empty for an ELF
    /// symbol that names no version).
    pub library: String,
    /// The symbol's name, or `#` and its ordinal.
    pub name: String,
}

/// What a Windows version resource says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionFound {
    /// The fixed file version, as four numbers.
    pub file_version: Option<String>,
    /// The fixed product version, as four numbers.
    pub product_version: Option<String>,
    /// Every string of the first string table, by key.
    pub strings: BTreeMap<String, String>,
}

/// What a Windows manifest says of how the program asks to be run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestFound {
    /// The assembly's name and version, as `name version`.
    pub identity: Option<String>,
    /// `asInvoker`, `highestAvailable`, `requireAdministrator`, or what
    /// else the file says.
    pub execution_level: Option<String>,
    /// Whether it asks for access to the user interface of higher
    /// programs.
    pub ui_access: Option<bool>,
}

/// Who signed a file, as far as it was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignerFound {
    /// `code_signature` (Mach-O) or `authenticode` (PE).
    pub form: String,
    /// The identifier a Mach-O code directory states.
    pub identifier: Option<String>,
    /// The team identifier a Mach-O code directory states.
    pub team: Option<String>,
    /// Whether the signature is ad hoc: made by no identity at all.
    pub ad_hoc: Option<bool>,
    /// How many bytes the signature takes in the file.
    pub bytes: u64,
}

/// What a `binary` finding holds: the plain facts of one file. Every text
/// in it came from the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinaryFound {
    /// `elf`, `pe`, `macho`, or `macho_fat`.
    pub format: String,
    /// The system it is for: `windows`, `macos`, `linux`, and so on; what
    /// the file does not say is `elf_osabi_N` or `macho_unspecified`.
    pub system: String,
    /// The processor, as the library names it.
    pub architecture: String,
    /// 32 or 64; 0 for a fat file.
    pub bits: u8,
    /// `little` or `big`; empty for a fat file.
    pub endian: String,
    /// `executable`, `library`, `object`, `core`, or `unknown`.
    pub kind: String,
    /// The subsystem of a PE file: `console`, `gui`, `native`, or a
    /// number.
    pub subsystem: Option<String>,
    /// The libraries it needs to be loaded.
    pub libraries: Vec<String>,
    /// The symbols it imports, cut at the bound.
    pub imports: Vec<ImportFound>,
    /// How many it imports, before the cut.
    pub imports_total: u64,
    /// The symbols it exports, cut at the bound.
    pub exports: Vec<String>,
    /// How many it exports, before the cut.
    pub exports_total: u64,
    /// The version resource of a PE file.
    pub version: Option<VersionFound>,
    /// The manifest of a PE file.
    pub manifest: Option<ManifestFound>,
    /// The names of the entitlements a Mach-O signature holds.
    pub entitlements: Vec<String>,
    /// Who signed it.
    pub signer: Option<SignerFound>,
    /// What was not read, by reason, with how many.
    pub unread: BTreeMap<String, u64>,
    /// The slices of a fat Mach-O file; empty for any other.
    pub slices: Vec<BinaryFound>,
}

/// A text from the file: redacted whole, then cut.
fn text(redact: &dyn Redact, raw: &str) -> String {
    let clean = redact.redact(raw);
    let clean: String = clean.chars().filter(|c| !c.is_control()).collect();
    clean.chars().take(MOST_TEXT).collect()
}

fn lossy(redact: &dyn Redact, raw: &[u8]) -> String {
    text(redact, &String::from_utf8_lossy(raw))
}

// ---- Reading numbers out of the bytes, never past their end. ----

#[derive(Clone, Copy)]
struct Bytes<'a> {
    data: &'a [u8],
    big: bool,
}

impl Bytes<'_> {
    fn uint(&self, at: u64, size: usize) -> Option<u64> {
        let at = usize::try_from(at).ok()?;
        let part = self.data.get(at..at.checked_add(size)?)?;
        let mut value = 0_u64;
        for (index, byte) in part.iter().enumerate() {
            let shift = if self.big { size - 1 - index } else { index };
            value |= u64::from(*byte) << (8 * shift);
        }
        Some(value)
    }
}

fn short(what: &str, at: u64, have: u64) -> Stop {
    Stop::Truncated(format!(
        "the {what} is at byte {at} and the file has {have} bytes"
    ))
}

/// How many bytes a file says it holds, as far as its headers say.
fn need(have: u64, what: &str, offset: u64, size: u64) -> Result<(), Stop> {
    let end = offset.checked_add(size).ok_or_else(|| {
        Stop::Malformed(format!(
            "the {what} runs past the largest offset a file has"
        ))
    })?;
    if end > have {
        return Err(Stop::Truncated(format!(
            "the {what} ends at byte {end} and the file has {have} bytes"
        )));
    }
    Ok(())
}

// ---- The kind of file. ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Elf,
    Pe,
    MachO,
    Fat,
}

fn shape(data: &[u8]) -> Result<Shape, Stop> {
    let Some(magic) = data.get(..4) else {
        return Err(Stop::Truncated(format!(
            "the file has {} bytes, fewer than the four of a magic number",
            data.len()
        )));
    };
    if magic == b"\x7fELF" {
        return Ok(Shape::Elf);
    }
    if magic.starts_with(b"MZ") {
        return Ok(Shape::Pe);
    }
    let be = u32::from_be_bytes([magic[0], magic[1], magic[2], magic[3]]);
    match be {
        0xfeed_face | 0xfeed_facf | 0xcefa_edfe | 0xcffa_edfe => Ok(Shape::MachO),
        0xcafe_babe | 0xcafe_babf | 0xbeba_feca | 0xbfba_feca => Ok(Shape::Fat),
        _ => Err(Stop::Kind(
            "it is not an ELF, a PE, or a Mach-O file: its first bytes are none of their magic numbers"
                .to_owned(),
        )),
    }
}

// ---- ELF. ----

fn elf_facts(data: &[u8], redact: &dyn Redact) -> Result<BinaryFound, Stop> {
    let have = data.len() as u64;
    let class = *data.get(4).ok_or_else(|| short("class byte", 4, have))?;
    let order = *data
        .get(5)
        .ok_or_else(|| short("byte-order byte", 5, have))?;
    let (wide, header) = match class {
        1 => (false, 52_u64),
        2 => (true, 64),
        _ => return Err(Stop::Malformed(format!("its ELF class is {class}"))),
    };
    let big = match order {
        1 => false,
        2 => true,
        _ => return Err(Stop::Malformed(format!("its ELF byte order is {order}"))),
    };
    need(have, "ELF header", 0, header)?;
    let b = Bytes { data, big };
    let at = |narrow: u64, wide_at: u64, size: usize| {
        b.uint(if wide { wide_at } else { narrow }, size)
            .ok_or_else(|| short("ELF header field", narrow, have))
    };
    let word = if wide { 8 } else { 4 };
    let phoff = at(28, 32, word)?;
    let shoff = at(32, 40, word)?;
    let phentsize = at(42, 54, 2)?;
    let phnum = at(44, 56, 2)?;
    let shentsize = at(46, 58, 2)?;
    let shnum = at(48, 60, 2)?;
    if phnum > 0 {
        need(have, "ELF program headers", phoff, phnum * phentsize)?;
    }
    if shnum > 0 {
        need(have, "ELF section headers", shoff, shnum * shentsize)?;
    }
    for index in 0..phnum {
        let entry = phoff + index * phentsize;
        let (offset, size) = if wide {
            (b.uint(entry + 8, 8), b.uint(entry + 32, 8))
        } else {
            (b.uint(entry + 4, 4), b.uint(entry + 16, 4))
        };
        let (Some(offset), Some(size)) = (offset, size) else {
            return Err(short("ELF program header", entry, have));
        };
        need(have, "ELF segment", offset, size)?;
    }
    for index in 0..shnum {
        let entry = shoff + index * shentsize;
        let kind = b.uint(entry + 4, 4);
        let (offset, size) = if wide {
            (b.uint(entry + 24, 8), b.uint(entry + 32, 8))
        } else {
            (b.uint(entry + 16, 4), b.uint(entry + 20, 4))
        };
        let (Some(kind), Some(offset), Some(size)) = (kind, offset, size) else {
            return Err(short("ELF section header", entry, have));
        };
        // A section that takes no room in the file has no bytes to be cut.
        if kind != 8 {
            need(have, "ELF section", offset, size)?;
        }
    }
    let osabi = data.get(7).copied().unwrap_or(0);
    let system = match osabi {
        0 => "unix_sysv".to_owned(),
        2 => "netbsd".to_owned(),
        3 => "linux".to_owned(),
        6 => "solaris".to_owned(),
        9 => "freebsd".to_owned(),
        12 => "openbsd".to_owned(),
        other => format!("elf_osabi_{other}"),
    };
    let mut found = if wide {
        let file =
            object::read::elf::ElfFile64::<object::Endianness>::parse(data).map_err(malformed)?;
        common(&file, "elf", &system, redact)?
    } else {
        let file =
            object::read::elf::ElfFile32::<object::Endianness>::parse(data).map_err(malformed)?;
        common(&file, "elf", &system, redact)?
    };
    // A position-independent program has the type of a shared object, and
    // is told from one by the interpreter it names (`PT_INTERP`, 3).
    let has_interpreter = (0..phnum).any(|index| b.uint(phoff + index * phentsize, 4) == Some(3));
    if b.uint(16, 2) == Some(3) && has_interpreter {
        "executable".clone_into(&mut found.kind);
    }
    found.signer = None;
    Ok(found)
}

fn malformed(error: object::read::Error) -> Stop {
    Stop::Malformed(format!("{error}"))
}

/// What every format gives through the library: the processor, the kind,
/// the libraries, and the symbols.
fn common<'data, O: Object<'data>>(
    file: &O,
    format: &str,
    system: &str,
    redact: &dyn Redact,
) -> Result<BinaryFound, Stop> {
    let mut unread: BTreeMap<String, u64> = BTreeMap::new();
    let mut libraries = Vec::new();
    match file.import_libraries() {
        Ok(found) => {
            for library in found {
                match library {
                    Ok(library) => {
                        if libraries.len() < MOST_LISTED {
                            libraries.push(lossy(redact, library.name()));
                        }
                    }
                    Err(_) => *unread.entry("library".to_owned()).or_default() += 1,
                }
            }
        }
        Err(_) => *unread.entry("libraries".to_owned()).or_default() += 1,
    }
    let mut imports = Vec::new();
    let mut imports_total = 0_u64;
    match file.imports() {
        Ok(found) => {
            for import in found {
                match import {
                    Ok(import) => {
                        imports_total += 1;
                        if imports.len() < MOST_LISTED {
                            let name = match import.name() {
                                object::read::NameOrOrdinal::Name(name) => lossy(redact, name),
                                object::read::NameOrOrdinal::Ordinal(ordinal) => {
                                    format!("#{ordinal}")
                                }
                            };
                            imports.push(ImportFound {
                                library: lossy(redact, import.library()),
                                name,
                            });
                        }
                    }
                    Err(_) => *unread.entry("import".to_owned()).or_default() += 1,
                }
            }
        }
        Err(_) => *unread.entry("imports".to_owned()).or_default() += 1,
    }
    let mut exports = Vec::new();
    let mut exports_total = 0_u64;
    match file.exports() {
        Ok(found) => {
            for export in found {
                match export {
                    Ok(export) => {
                        exports_total += 1;
                        if exports.len() < MOST_LISTED {
                            exports.push(match export.name() {
                                object::read::NameOrOrdinal::Name(name) => lossy(redact, name),
                                object::read::NameOrOrdinal::Ordinal(ordinal) => {
                                    format!("#{ordinal}")
                                }
                            });
                        }
                    }
                    Err(_) => *unread.entry("export".to_owned()).or_default() += 1,
                }
            }
        }
        Err(_) => *unread.entry("exports".to_owned()).or_default() += 1,
    }
    Ok(BinaryFound {
        format: format.to_owned(),
        system: system.to_owned(),
        architecture: format!("{:?}", file.architecture()).to_ascii_lowercase(),
        bits: if file.is_64() { 64 } else { 32 },
        endian: match file.endianness() {
            object::Endianness::Little => "little",
            object::Endianness::Big => "big",
        }
        .to_owned(),
        kind: match file.kind() {
            ObjectKind::Executable => "executable",
            ObjectKind::Dynamic => "library",
            ObjectKind::Relocatable => "object",
            ObjectKind::Core => "core",
            _ => "unknown",
        }
        .to_owned(),
        subsystem: None,
        libraries,
        imports,
        imports_total,
        exports,
        exports_total,
        version: None,
        manifest: None,
        entitlements: Vec::new(),
        signer: None,
        unread,
        slices: Vec::new(),
    })
}

// ---- PE. ----

fn pe_facts(data: &[u8], redact: &dyn Redact) -> Result<BinaryFound, Stop> {
    let have = data.len() as u64;
    let b = Bytes { data, big: false };
    need(have, "DOS header", 0, 64)?;
    let nt = b
        .uint(0x3c, 4)
        .ok_or_else(|| short("DOS header", 0x3c, have))?;
    need(have, "PE signature", nt, 4)?;
    if data.get(nt as usize..nt as usize + 4) != Some(b"PE\0\0") {
        return Err(Stop::Kind(
            "it starts with `MZ` and holds no PE signature at the place its DOS header gives: it is a DOS or another older format"
                .to_owned(),
        ));
    }
    need(have, "PE file header", nt + 4, 20)?;
    let sections = b
        .uint(nt + 6, 2)
        .ok_or_else(|| short("section count", nt, have))?;
    let optional = b
        .uint(nt + 20, 2)
        .ok_or_else(|| short("optional header size", nt, have))?;
    need(have, "PE optional header", nt + 24, optional)?;
    let table = nt + 24 + optional;
    need(have, "PE section table", table, sections * 40)?;
    for index in 0..sections {
        let entry = table + index * 40;
        let (Some(size), Some(offset)) = (b.uint(entry + 16, 4), b.uint(entry + 20, 4)) else {
            return Err(short("PE section header", entry, have));
        };
        if offset != 0 || size != 0 {
            need(have, "PE section", offset, size)?;
        }
    }
    let magic = b
        .uint(nt + 24, 2)
        .ok_or_else(|| short("optional header", nt + 24, have))?;
    let wide = match magic {
        0x10b => false,
        0x20b => true,
        other => {
            return Err(Stop::Malformed(format!(
                "its optional header has the magic number {other:#x}, neither PE32 nor PE32+"
            )));
        }
    };
    let subsystem = b.uint(nt + 24 + 68, 2).map(|n| match n {
        1 => "native".to_owned(),
        2 => "gui".to_owned(),
        3 => "console".to_owned(),
        other => other.to_string(),
    });
    let mut found;
    let (version, manifest, signer);
    if wide {
        let file = PeFile64::parse(data).map_err(malformed)?;
        found = common(&file, "pe", "windows", redact)?;
        (version, manifest, signer) = pe_resources(
            &file.data_directories(),
            &file.section_table(),
            data,
            redact,
        )?;
    } else {
        let file = PeFile32::parse(data).map_err(malformed)?;
        found = common(&file, "pe", "windows", redact)?;
        (version, manifest, signer) = pe_resources(
            &file.data_directories(),
            &file.section_table(),
            data,
            redact,
        )?;
    }
    found.subsystem = subsystem;
    found.version = version;
    found.manifest = manifest;
    found.signer = signer;
    Ok(found)
}

type PeParts = (
    Option<VersionFound>,
    Option<ManifestFound>,
    Option<SignerFound>,
);

fn pe_resources<'data>(
    directories: &object::read::pe::DataDirectories<'data>,
    sections: &object::read::pe::SectionTable<'data>,
    data: &'data [u8],
    redact: &dyn Redact,
) -> Result<PeParts, Stop> {
    let have = data.len() as u64;
    let signer = match directories.get(object::pe::IMAGE_DIRECTORY_ENTRY_SECURITY) {
        Some(entry) if entry.size.get(object::LittleEndian) > 0 => {
            let offset = u64::from(entry.virtual_address.get(object::LittleEndian));
            let size = u64::from(entry.size.get(object::LittleEndian));
            need(have, "certificate table", offset, size)?;
            Some(SignerFound {
                form: "authenticode".to_owned(),
                identifier: None,
                team: None,
                ad_hoc: None,
                bytes: size,
            })
        }
        _ => None,
    };
    let Some(directory) = directories
        .resource_directory(data, sections)
        .map_err(malformed)?
    else {
        return Ok((None, None, signer));
    };
    let mut budget = MOST_RESOURCE_ENTRIES;
    let version = find_resource(directory, 16, &mut budget)?
        .and_then(|(rva, size)| resource_bytes(sections, data, rva, size))
        .and_then(|bytes| version_of(bytes, redact));
    let manifest = find_resource(directory, 24, &mut budget)?
        .and_then(|(rva, size)| resource_bytes(sections, data, rva, size))
        .and_then(|bytes| manifest_of(bytes, redact));
    Ok((version, manifest, signer))
}

fn resource_bytes<'data>(
    sections: &object::read::pe::SectionTable<'data>,
    data: &'data [u8],
    rva: u32,
    size: u32,
) -> Option<&'data [u8]> {
    let from = sections.pe_data_at(data, rva)?;
    let size = usize::try_from(size).ok()?.min(MOST_RESOURCE_BYTES);
    from.get(..size)
}

/// The first data entry under the resource type `wanted`, by walking the
/// three levels of the resource tree. A table that an entry above it
/// already is, is a cycle.
fn find_resource(
    directory: object::read::pe::ResourceDirectory<'_>,
    wanted: u16,
    budget: &mut usize,
) -> Result<Option<(u32, u32)>, Stop> {
    let root = directory.root().map_err(malformed)?;
    let mut path = vec![0_u32];
    for entry in root.entries {
        if entry.has_name() {
            continue;
        }
        let id = (entry.name_or_id_value() & 0xffff) as u16;
        if id != wanted {
            continue;
        }
        if let Some(found) = descend(directory, entry, &mut path, budget)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

trait EntryValue {
    fn name_or_id_value(&self) -> u32;
}

impl EntryValue for object::pe::ImageResourceDirectoryEntry {
    fn name_or_id_value(&self) -> u32 {
        self.name_or_id.get(object::LittleEndian)
    }
}

fn descend(
    directory: object::read::pe::ResourceDirectory<'_>,
    entry: &object::pe::ImageResourceDirectoryEntry,
    path: &mut Vec<u32>,
    budget: &mut usize,
) -> Result<Option<(u32, u32)>, Stop> {
    if *budget == 0 {
        return Err(Stop::Malformed(format!(
            "its resource tree holds more than {MOST_RESOURCE_ENTRIES} entries"
        )));
    }
    *budget -= 1;
    if entry.is_table() {
        let offset = entry.data_offset();
        if path.contains(&offset) {
            return Err(Stop::Cyclic(format!(
                "a table of its resource tree, at offset {offset}, is reached again from below itself"
            )));
        }
        if path.len() >= 3 {
            return Err(Stop::Malformed(
                "its resource tree is deeper than the three levels of type, name, and language"
                    .to_owned(),
            ));
        }
        let table = entry
            .data(directory)
            .map_err(malformed)?
            .table()
            .ok_or_else(|| Stop::Malformed("a resource table is not one".to_owned()))?;
        path.push(offset);
        for child in table.entries {
            if let Some(found) = descend(directory, child, path, budget)? {
                path.pop();
                return Ok(Some(found));
            }
        }
        path.pop();
        Ok(None)
    } else {
        let data = entry
            .data(directory)
            .map_err(malformed)?
            .data()
            .ok_or_else(|| Stop::Malformed("a resource entry is not data".to_owned()))?;
        Ok(Some((
            data.offset_to_data.get(object::LittleEndian),
            data.size.get(object::LittleEndian),
        )))
    }
}

// ---- The version resource: VS_VERSIONINFO. ----

/// One block of a version resource: its key, its value, and the part
/// where its children are.
struct Block<'a> {
    key: String,
    value: &'a [u8],
    text_value: bool,
    children: &'a [u8],
    next: usize,
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn word(data: &[u8], at: usize) -> Option<usize> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(usize::from(u16::from_le_bytes([bytes[0], bytes[1]])))
}

fn utf16(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// The sibling blocks that fill `data`, from its start, as far as each is
/// well formed.
fn blocks(data: &[u8]) -> Vec<Block<'_>> {
    let mut found = Vec::new();
    let mut at = 0;
    while found.len() < MOST_VERSION_STRINGS && at < data.len() {
        let Some(next) = block(data, at, data.len()) else {
            break;
        };
        let after = next.next;
        found.push(next);
        if after <= at {
            break;
        }
        at = after;
    }
    found
}

/// The block at `at` of `data`, whose end is not past `limit`.
fn block(data: &[u8], at: usize, limit: usize) -> Option<Block<'_>> {
    let length = word(data, at)?;
    let value_length = word(data, at + 2)?;
    let kind = word(data, at + 4)?;
    let end = at.checked_add(length)?;
    if length < 6 || end > limit || end > data.len() {
        return None;
    }
    // The key: UTF-16, ended by a zero unit, inside the block.
    let mut key_end = at + 6;
    loop {
        let unit = data.get(key_end..key_end + 2)?;
        if key_end + 2 > end {
            return None;
        }
        key_end += 2;
        if unit == [0, 0] {
            break;
        }
    }
    let key = utf16(&data[at + 6..key_end]);
    let value_at = align4(key_end);
    let text_value = kind == 1;
    let value_bytes = if text_value {
        value_length.checked_mul(2)?
    } else {
        value_length
    };
    let value_end = value_at.checked_add(value_bytes)?.min(end);
    let value = data.get(value_at.min(end)..value_end)?;
    let children_at = align4(value_end).min(end);
    Some(Block {
        key,
        value,
        text_value,
        children: data.get(children_at..end)?,
        next: align4(end),
    })
}

fn four(ms: u64, ls: u64) -> String {
    format!(
        "{}.{}.{}.{}",
        (ms >> 16) & 0xffff,
        ms & 0xffff,
        (ls >> 16) & 0xffff,
        ls & 0xffff
    )
}

fn version_of(data: &[u8], redact: &dyn Redact) -> Option<VersionFound> {
    let root = block(data, 0, data.len())?;
    if root.key != "VS_VERSION_INFO" {
        return None;
    }
    let b = Bytes {
        data: root.value,
        big: false,
    };
    let (file_version, product_version) = match b.uint(0, 4) {
        Some(0xfeef_04bd) if root.value.len() >= 24 => (
            Some(four(b.uint(8, 4)?, b.uint(12, 4)?)),
            Some(four(b.uint(16, 4)?, b.uint(20, 4)?)),
        ),
        _ => (None, None),
    };
    let mut strings = BTreeMap::new();
    // The first string table of the string file info: its languages are
    // told apart by nothing a person reads, so the first is taken.
    let table = blocks(root.children)
        .into_iter()
        .find(|child| child.key == "StringFileInfo")
        .and_then(|info| blocks(info.children).into_iter().next());
    if let Some(table) = table {
        for entry in blocks(table.children) {
            if entry.text_value && strings.len() < MOST_VERSION_STRINGS {
                strings.insert(text(redact, &entry.key), text(redact, &utf16(entry.value)));
            }
        }
    }
    Some(VersionFound {
        file_version,
        product_version,
        strings,
    })
}

// ---- The manifest. ----

fn manifest_of(data: &[u8], redact: &dyn Redact) -> Option<ManifestFound> {
    let data = data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(data);
    let source = std::str::from_utf8(data).ok()?;
    let mut reader = Reader::from_str(source);
    let mut found = ManifestFound {
        identity: None,
        execution_level: None,
        ui_access: None,
    };
    for _ in 0..MOST_EVENTS {
        match reader.read_event().ok()? {
            Event::Eof => break,
            // A document type is refused: nothing is expanded or fetched.
            Event::DocType(_) => return None,
            Event::Start(element) | Event::Empty(element) => {
                let name = element.local_name().into_inner();
                let attribute = |wanted: &str| -> Option<String> {
                    element
                        .attributes()
                        .flatten()
                        .find(|a| a.key.local_name().into_inner() == wanted)
                        .and_then(|a| {
                            a.normalized_value_with(
                                quick_xml::XmlVersion::Implicit1_0,
                                1,
                                quick_xml::escape::resolve_predefined_entity,
                            )
                            .ok()
                        })
                        .map(|value| text(redact, &value))
                };
                if name == "assemblyIdentity" && found.identity.is_none() {
                    let name = attribute("name");
                    let version = attribute("version");
                    found.identity = match (name, version) {
                        (Some(name), Some(version)) => Some(format!("{name} {version}")),
                        (Some(name), None) => Some(name),
                        _ => None,
                    };
                } else if name == "requestedExecutionLevel" {
                    found.execution_level = attribute("level");
                    found.ui_access =
                        attribute("uiAccess").map(|value| value.eq_ignore_ascii_case("true"));
                }
            }
            _ => {}
        }
    }
    Some(found)
}

// ---- Mach-O. ----

/// What the walk of a Mach-O file's load commands found.
struct Commands {
    platform: Option<String>,
    signature: Option<(u64, u64)>,
}

fn macho_commands(data: &[u8]) -> Result<(Commands, bool, bool), Stop> {
    let have = data.len() as u64;
    need(have, "Mach-O magic", 0, 4)?;
    let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let (big, wide) = match magic {
        0xfeed_face => (true, false),
        0xfeed_facf => (true, true),
        0xcefa_edfe => (false, false),
        _ => (false, true),
    };
    let b = Bytes { data, big };
    let header = if wide { 32_u64 } else { 28 };
    need(have, "Mach-O header", 0, header)?;
    let commands = b.uint(16, 4).ok_or_else(|| short("header", 16, have))?;
    let size = b.uint(20, 4).ok_or_else(|| short("header", 20, have))?;
    need(have, "Mach-O load commands", header, size)?;
    let mut at = header;
    let end = header + size;
    let mut found = Commands {
        platform: None,
        signature: None,
    };
    for _ in 0..commands {
        let (Some(cmd), Some(cmdsize)) = (b.uint(at, 4), b.uint(at + 4, 4)) else {
            return Err(Stop::Malformed(
                "a Mach-O load command is cut by the end of its area".to_owned(),
            ));
        };
        if cmdsize < 8 || at + cmdsize > end {
            return Err(Stop::Malformed(
                "a Mach-O load command is smaller than its header or passes its area".to_owned(),
            ));
        }
        match cmd {
            // LC_SEGMENT, LC_SEGMENT_64
            0x1 | 0x19 => {
                let (offset, filesize) = if cmd == 0x19 {
                    (b.uint(at + 40, 8), b.uint(at + 48, 8))
                } else {
                    (b.uint(at + 32, 4), b.uint(at + 36, 4))
                };
                let (Some(offset), Some(filesize)) = (offset, filesize) else {
                    return Err(short("segment command", at, have));
                };
                need(have, "Mach-O segment", offset, filesize)?;
            }
            // LC_CODE_SIGNATURE
            0x1d => {
                let (Some(offset), Some(length)) = (b.uint(at + 8, 4), b.uint(at + 12, 4)) else {
                    return Err(short("code-signature command", at, have));
                };
                need(have, "Mach-O code signature", offset, length)?;
                found.signature = Some((offset, length));
            }
            // LC_BUILD_VERSION
            0x32 => {
                if let Some(platform) = b.uint(at + 8, 4) {
                    found.platform = Some(platform_name(platform).to_owned());
                }
            }
            // LC_VERSION_MIN_MACOSX, _IPHONEOS, _WATCHOS, _TVOS
            0x24 | 0x25 | 0x30 | 0x2f if found.platform.is_none() => {
                found.platform = Some(
                    match cmd {
                        0x24 => "macos",
                        0x25 => "ios",
                        0x30 => "watchos",
                        _ => "tvos",
                    }
                    .to_owned(),
                );
            }
            _ => {}
        }
        at += cmdsize;
    }
    Ok((found, big, wide))
}

fn platform_name(platform: u64) -> &'static str {
    match platform {
        1 => "macos",
        2 => "ios",
        3 => "tvos",
        4 => "watchos",
        5 => "bridgeos",
        6 => "maccatalyst",
        7 => "ios_simulator",
        8 => "tvos_simulator",
        9 => "watchos_simulator",
        10 => "driverkit",
        11 => "visionos",
        12 => "visionos_simulator",
        _ => "macho_platform_unknown",
    }
}

fn macho_facts(data: &[u8], redact: &dyn Redact) -> Result<BinaryFound, Stop> {
    let (commands, _, wide) = macho_commands(data)?;
    let system = commands
        .platform
        .clone()
        .unwrap_or_else(|| "macho_unspecified".to_owned());
    let mut found = if wide {
        let file = object::read::macho::MachOFile64::<object::Endianness>::parse(data)
            .map_err(malformed)?;
        common(&file, "macho", &system, redact)?
    } else {
        let file = object::read::macho::MachOFile32::<object::Endianness>::parse(data)
            .map_err(malformed)?;
        common(&file, "macho", &system, redact)?
    };
    if let Some((offset, length)) = commands.signature {
        let (start, stop) = (offset as usize, (offset + length) as usize);
        let signature = data.get(start..stop).ok_or_else(|| {
            Stop::Truncated("the code signature is past the end of the file".to_owned())
        })?;
        let (signer, entitlements) = macho_signature(signature, length, redact, &mut found.unread);
        found.signer = signer;
        found.entitlements = entitlements;
    }
    Ok(found)
}

fn macho_signature(
    data: &[u8],
    bytes: u64,
    redact: &dyn Redact,
    unread: &mut BTreeMap<String, u64>,
) -> (Option<SignerFound>, Vec<String>) {
    let mut count = |why: &str| *unread.entry(why.to_owned()).or_default() += 1;
    let Ok(signature) = CodeSignature::parse(data) else {
        count("code_signature");
        return (None, Vec::new());
    };
    let mut signer = SignerFound {
        form: "code_signature".to_owned(),
        identifier: None,
        team: None,
        ad_hoc: None,
        bytes,
    };
    let mut entitlements = Vec::new();
    for blob in signature.blobs() {
        let Ok(blob) = blob else {
            count("code_signature_blob");
            continue;
        };
        match blob.magic() {
            macho::CSMAGIC_CODEDIRECTORY => match blob.code_directory() {
                Ok(Some(directory)) => {
                    signer.ad_hoc = Some(
                        directory
                            .header()
                            .flags
                            .get(object::BigEndian)
                            .contains_ad_hoc(),
                    );
                    signer.identifier = directory.ident().ok().map(|id| lossy(redact, id));
                    signer.team = directory
                        .team_id()
                        .ok()
                        .flatten()
                        .map(|team| lossy(redact, team));
                }
                _ => count("code_directory"),
            },
            macho::CSMAGIC_EMBEDDED_ENTITLEMENTS => {
                match entitlement_names(blob.contents(), redact) {
                    Some(names) => entitlements = names,
                    None => count("entitlements"),
                }
            }
            _ => {}
        }
    }
    (Some(signer), entitlements)
}

trait AdHoc {
    fn contains_ad_hoc(self) -> bool;
}

impl AdHoc for macho::CsFlags {
    fn contains_ad_hoc(self) -> bool {
        self.0 & 0x2 != 0
    }
}

/// The keys of the top-level dictionary of an entitlements property list,
/// in the order the file gives them.
fn entitlement_names(data: &[u8], redact: &dyn Redact) -> Option<Vec<String>> {
    let source = std::str::from_utf8(data).ok()?;
    let mut reader = Reader::from_str(source);
    let mut depth = 0_usize;
    let mut in_key = false;
    let mut names = Vec::new();
    for _ in 0..MOST_EVENTS {
        match reader.read_event().ok()? {
            Event::Eof => break,
            Event::DocType(_) => {
                // The usual header names a public document type; it is not
                // fetched and no entity is read from it.
                continue;
            }
            Event::Start(element) => {
                let name = element.local_name().into_inner();
                if name == "dict" || name == "array" {
                    depth += 1;
                } else if name == "key" && depth == 1 {
                    in_key = true;
                }
            }
            Event::End(element) => {
                let name = element.local_name().into_inner();
                if name == "dict" || name == "array" {
                    depth = depth.saturating_sub(1);
                } else if name == "key" {
                    in_key = false;
                }
            }
            Event::Text(part) if in_key && names.len() < MOST_KEYS => {
                let key = part.xml_content(quick_xml::XmlVersion::Implicit1_0);
                names.push(text(redact, &key));
            }
            _ => {}
        }
    }
    Some(names)
}

fn fat_facts(data: &[u8], redact: &dyn Redact) -> Result<BinaryFound, Stop> {
    let arches = parse_arches(data)?;
    if arches.is_empty() {
        return Err(Stop::Malformed("a fat file lists no slice".to_owned()));
    }
    if arches.len() > MOST_SLICES {
        return Err(Stop::Malformed(format!(
            "a fat file lists {} slices, more than the {MOST_SLICES} that are read",
            arches.len()
        )));
    }
    let have = data.len() as u64;
    let mut slices = Vec::new();
    for (offset, size) in arches {
        if offset == 0 {
            return Err(Stop::Cyclic(
                "a slice of a fat file starts at the file's own first byte, the fat header"
                    .to_owned(),
            ));
        }
        need(have, "fat slice", offset, size)?;
        let slice = &data[offset as usize..(offset + size) as usize];
        match shape(slice)? {
            Shape::MachO => slices.push(macho_facts(slice, redact)?),
            Shape::Fat => {
                return Err(Stop::Cyclic(
                    "a slice of a fat file is itself a fat file".to_owned(),
                ));
            }
            _ => {
                return Err(Stop::Malformed(
                    "a slice of a fat file is not a Mach-O file".to_owned(),
                ));
            }
        }
    }
    let first = &slices[0];
    Ok(BinaryFound {
        format: "macho_fat".to_owned(),
        system: first.system.clone(),
        architecture: String::new(),
        bits: 0,
        endian: String::new(),
        kind: first.kind.clone(),
        subsystem: None,
        libraries: Vec::new(),
        imports: Vec::new(),
        imports_total: 0,
        exports: Vec::new(),
        exports_total: 0,
        version: None,
        manifest: None,
        entitlements: Vec::new(),
        signer: None,
        unread: BTreeMap::new(),
        slices,
    })
}

/// The offset and size of each slice a fat header lists, whichever width
/// of entry it has.
fn parse_arches(data: &[u8]) -> Result<Vec<(u64, u64)>, Stop> {
    let have = data.len() as u64;
    let b = Bytes { data, big: true };
    let wide = matches!(b.uint(0, 4), Some(0xcafe_babf | 0xbfba_feca));
    let count = b.uint(4, 4).ok_or_else(|| short("fat header", 4, have))?;
    if count > 4096 {
        // Java class files share the first magic number of a fat file.
        return Err(Stop::Kind(
            "it starts like a fat Mach-O file and lists more slices than a fat file has".to_owned(),
        ));
    }
    let entry = if wide { 32_u64 } else { 20 };
    need(have, "fat slice list", 8, count * entry)?;
    let mut found = Vec::new();
    for index in 0..count {
        let at = 8 + index * entry;
        let (offset, size) = if wide {
            (b.uint(at + 8, 8), b.uint(at + 16, 8))
        } else {
            (b.uint(at + 8, 4), b.uint(at + 12, 4))
        };
        let (Some(offset), Some(size)) = (offset, size) else {
            return Err(short("fat slice entry", at, have));
        };
        found.push((offset, size));
    }
    Ok(found)
}

/// The plain facts of the bytes of one file, which are never run.
///
/// # Errors
///
/// A [`Stop`] for a file of no known kind, one cut short, one that leads
/// back to itself, and one that is not well formed.
pub(crate) fn facts(data: &[u8], redact: &dyn Redact) -> Result<BinaryFound, Stop> {
    match shape(data)? {
        Shape::Elf => elf_facts(data, redact),
        Shape::Pe => pe_facts(data, redact),
        Shape::MachO => macho_facts(data, redact),
        Shape::Fat => fat_facts(data, redact),
    }
}

// ---- The discoverer. ----

use std::sync::Arc;

use a2m_core::{
    Coverage, Digest, EvidenceLevel, Failure, Finding, LimitationCode, PackPayload, ProducedBy,
    Source, Subject, Values,
};
use a2m_logs::Clock;
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, SurfaceDescription,
};

use crate::codes::{self, BINARY_DISCOVERER, PACK, SURFACE_KIND};
use crate::json::{self, Json};

/// The name a binary is handed to the scout under: a file of the project's
/// `inputs` folder, never the program's own path.
pub const BINARY: &str = "binary";

impl Stop {
    fn failure(self) -> Failure {
        match self {
            Self::Kind(why) => codes::failure(
                &codes::row::BINARY_KIND_UNKNOWN,
                format!(
                    "The file handed to the scout as `{BINARY}` has no kind this reader has a rule for: {why}."
                ),
                "Hand a program of one of the three kinds the reader has a rule for: an ELF, a PE, or a Mach-O file.",
            ),
            Self::Truncated(why) => codes::failure(
                &codes::row::BINARY_TRUNCATED,
                format!("The file handed to the scout as `{BINARY}` is cut short: {why}."),
                "Copy the whole file again; a file the copy cut short cannot be read.",
            ),
            Self::Cyclic(why) => codes::failure(
                &codes::row::BINARY_CYCLIC,
                format!("The file handed to the scout as `{BINARY}` leads back to itself: {why}."),
                "Do not use this file: a program that is whole never leads back to a part it is being read from.",
            ),
            Self::Malformed(why) => codes::failure(
                &codes::row::BINARY_MALFORMED,
                format!("The file handed to the scout as `{BINARY}` is not well formed: {why}."),
                "Hand the program as it was built; a file a tool has changed by hand may no longer be one.",
            ),
        }
    }
}

/// The discoverer of the binary's plain facts. It starts nothing.
pub(crate) struct BinaryDiscoverer {
    pub clock: Arc<dyn Clock>,
}

impl BinaryDiscoverer {
    fn look(
        &self,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let Some(path) = scope.paths.get(BINARY) else {
            return Ok(DiscoveryReport::default());
        };
        let data = path.read(scope.bounds.bytes)?;
        let found = facts(&data, scope.redact).map_err(Stop::failure)?;
        let truncated = found.imports_total > found.imports.len() as u64
            || found.exports_total > found.exports.len() as u64;
        let unread: u64 = found.unread.values().sum();
        let encoded = PackPayload::encode(&found)?;
        let mut finding = Finding::about(
            Subject::File {
                digest: Digest::of(&data),
                format: format!("binary_{}", found.format),
                part: None,
            },
            codes::binary_finding_kind(),
            String::from_utf8_lossy(encoded.as_bytes()).into_owned(),
            Source::Artifact,
            ProducedBy {
                provider: PACK.to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                component: None,
            },
            self.clock.now(),
        )
        .level(EvidenceLevel::Inferred)
        .coverage(if truncated || unread > 0 {
            Coverage::Partial
        } else {
            Coverage::Complete
        })
        .limitation(
            LimitationCode::NotExercised,
            "The file was read as bytes and never run or loaded. What it imports and exports is what its tables say; whether the program works, and what any command of it does, is not known.",
        );
        if truncated {
            finding = finding.limitation(
                LimitationCode::Truncated,
                "The file lists more imports or exports than one finding holds; the totals say how many.",
            );
        }
        if found
            .signer
            .as_ref()
            .is_some_and(|signer| signer.form == "authenticode")
        {
            finding = finding.limitation(
                LimitationCode::UnverifiedComponent,
                "The file carries an Authenticode signature. Only that it is there and how large it is was read: who signed it was not, and nothing was checked.",
            );
        } else if found.signer.is_some() {
            finding = finding.limitation(
                LimitationCode::UnverifiedComponent,
                "The identifier and team a code signature states were read from the file; the signature itself was not checked against anything.",
            );
        }
        out.found(finding.build().map_err(|_| {
            a2m_core::codes::row::FINDING_INVALID.failure(
                "A finding about a binary could not be made.",
                "Report this to the command pack's author.",
            )
        })?)?;
        Ok(DiscoveryReport {
            coverage: Coverage::default(),
            bounds_hit: if truncated {
                vec!["listed".to_owned()]
            } else {
                Vec::new()
            },
            dropped: unread,
        })
    }
}

impl Discoverer for BinaryDiscoverer {
    fn name(&self) -> DiscovererName {
        DiscovererName::new(BINARY_DISCOVERER)
            .unwrap_or_else(|_| unreachable!("`binary` is a well-formed discoverer name"))
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == SURFACE_KIND
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(std::future::ready(self.look(scope, out)))
    }
}

/// Puts a `binary` finding so that an agent can read it. What the file
/// said is untrusted text and is listed apart.
pub(crate) struct BinaryDescriber {
    pub redact: Arc<dyn Redact>,
}

fn field<'a>(fields: &'a [(String, Json)], name: &str) -> Option<&'a Json> {
    fields.iter().find(|(key, _)| key == name).map(|(_, v)| v)
}

fn as_text(value: Option<&Json>) -> &str {
    match value {
        Some(Json::Text(text)) => text,
        _ => "",
    }
}

fn as_number(value: Option<&Json>) -> &str {
    match value {
        Some(Json::Number(digits)) => digits,
        _ => "0",
    }
}

impl Describer for BinaryDescriber {
    fn describe(&self, findings: &[Finding], _form: DescribeForm) -> Description {
        let mut untrusted = Vec::new();
        let mut text = String::new();
        let mut count = 0_i64;
        for finding in findings
            .iter()
            .filter(|finding| *finding.kind() == codes::binary_finding_kind())
        {
            let Ok(Json::Object(fields)) = json::parse(finding.found()) else {
                continue;
            };
            count += 1;
            let line = format!(
                "A {} file for {} ({}, {}): {} imports, {} exports. It was read as bytes and never run.",
                as_text(field(&fields, "format")),
                as_text(field(&fields, "system")),
                as_text(field(&fields, "architecture")),
                as_text(field(&fields, "kind")),
                as_number(field(&fields, "imports_total")),
                as_number(field(&fields, "exports_total")),
            );
            text.push_str(&self.redact.redact(&line));
            text.push(' ');
            if let Some(Json::List(libraries)) = field(&fields, "libraries") {
                for library in libraries {
                    if let Json::Text(name) = library {
                        untrusted.push(self.redact.redact(name));
                    }
                }
            }
        }
        Description {
            text: text.trim_end().to_owned(),
            data: Values::new().with("binaries", count),
            untrusted,
        }
    }
}
