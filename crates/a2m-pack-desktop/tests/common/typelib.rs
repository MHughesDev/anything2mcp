//! A synthetic COM type library in the MSFT form, built byte by byte: the
//! smallest file the reader takes that shows what it must tell apart. It
//! describes the imaginary `ExampleDeskLib` with one dispatch interface,
//! `Orders`, whose members are, in the file's order:
//!
//! ```text
//! property get  Total   -> Long      (memid 1)
//! property get  Label   -> String    (memid 2)
//! property put  Label   (value: String)
//! function      Remove  (order: String) -> Boolean   (memid 3)
//! ```
//!
//! The layout is the format's (a header, a table of type-info offsets, a
//! directory of fifteen segments, then the segments, then the function
//! block of the one type), written here from the format's own description,
//! not taken from any real library.

#![allow(dead_code)]

const HEADER: usize = 0x54;
const TYPEINFO_SIZE: usize = 0x64;
const GUID_ENTRY: usize = 24;

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_i32(bytes: &mut [u8], at: usize, value: i32) {
    put_u32(bytes, at, u32::from_le_bytes(value.to_le_bytes()));
}

/// One name-table entry: the header and the name, padded to four bytes.
fn name_entry(hreftype: i32, name: &str) -> Vec<u8> {
    let mut entry = Vec::new();
    entry.extend_from_slice(&hreftype.to_le_bytes());
    entry.extend_from_slice(&(-1_i32).to_le_bytes());
    entry.push(u8::try_from(name.len()).unwrap());
    entry.push(0);
    entry.extend_from_slice(&0_u16.to_le_bytes());
    entry.extend_from_slice(name.as_bytes());
    while entry.len() % 4 != 0 {
        entry.push(0);
    }
    entry
}

/// A function record: `invoke` is the invoke kind (1 function, 2 property
/// get, 4 property put), `returns` a base variant type, `params` the base
/// variant type and name offset of each parameter.
fn func_record(index: u32, invoke: u32, returns: u32, params: &[(u32, i32)]) -> Vec<u8> {
    let size = 0x18 + 12 * params.len();
    let mut record = vec![0_u8; size];
    put_u32(&mut record, 0, u32::try_from(size).unwrap() | (index << 16));
    // A base type is written in the type field itself, with the high bit.
    put_u32(&mut record, 4, 0x8000_0000 | returns);
    // FUNC_DISPATCH, the invoke kind, and the standard calling convention.
    put_u32(&mut record, 0x10, 4 | (invoke << 3) | (4 << 8));
    record[0x14..0x16].copy_from_slice(&i16::try_from(params.len()).unwrap().to_le_bytes());
    for (position, (vt, name)) in params.iter().enumerate() {
        let at = 0x18 + position * 12;
        put_u32(&mut record, at, 0x8000_0000 | vt);
        put_i32(&mut record, at + 4, *name);
        // An input parameter.
        put_u32(&mut record, at + 8, 1);
    }
    record
}

/// The library, as bytes.
pub fn example_typelib() -> Vec<u8> {
    // Names, in the table, with the offsets they will have.
    let mut names = Vec::new();
    let offset = |names: &mut Vec<u8>, hreftype: i32, name: &str| {
        let at = i32::try_from(names.len()).unwrap();
        names.extend(name_entry(hreftype, name));
        at
    };
    let lib_name = offset(&mut names, -1, "ExampleDeskLib");
    let ti_name = offset(&mut names, 0, "Orders");
    let total = offset(&mut names, 0, "Total");
    let label = offset(&mut names, 0, "Label");
    let remove = offset(&mut names, 0, "Remove");
    let order = offset(&mut names, 0, "order");
    let value = offset(&mut names, 0, "value");

    // The function block: the records, then the member ids, then the name
    // offsets, one of each for every member.
    let records = [
        func_record(0, 2, 3, &[]),
        func_record(1, 2, 8, &[]),
        func_record(2, 4, 24, &[(8, value)]),
        func_record(3, 1, 11, &[(8, order)]),
    ];
    let mut block = Vec::new();
    let size: usize = records.iter().map(Vec::len).sum();
    block.extend_from_slice(&u32::try_from(size).unwrap().to_le_bytes());
    for record in &records {
        block.extend_from_slice(record);
    }
    for memid in [1_i32, 2, 2, 3] {
        block.extend_from_slice(&memid.to_le_bytes());
    }
    for name in [total, label, label, remove] {
        block.extend_from_slice(&name.to_le_bytes());
    }

    // Where everything goes.
    let segdir = HEADER + 4;
    let typeinfo_at = segdir + 15 * 16;
    let guids_at = typeinfo_at + TYPEINFO_SIZE;
    let names_at = guids_at + 2 * GUID_ENTRY;
    let block_at = names_at + names.len();
    let total_len = block_at + block.len();

    let mut file = vec![0_u8; total_len];
    file[0..4].copy_from_slice(b"MSFT");
    put_u32(&mut file, 0x04, 0x0001_0002);
    put_i32(&mut file, 0x08, 0);
    put_u32(&mut file, 0x0C, 0x409);
    put_u32(&mut file, 0x10, 0x409);
    // Win32, and no help library.
    put_u32(&mut file, 0x14, 1);
    put_u32(&mut file, 0x18, 1);
    put_u32(&mut file, 0x20, 1);
    put_i32(&mut file, 0x24, -1);
    put_i32(&mut file, 0x38, lib_name);
    put_i32(&mut file, 0x3C, -1);
    put_i32(&mut file, 0x40, -1);
    // The offset table: one type, at offset 0 of its segment.
    put_i32(&mut file, HEADER, 0);
    // The segment directory: three segments, the rest absent.
    for index in 0..15 {
        let at = segdir + index * 16;
        put_i32(&mut file, at, -1);
    }
    let mut segment = |index: usize, offset: usize, length: usize| {
        let at = segdir + index * 16;
        put_i32(&mut file, at, i32::try_from(offset).unwrap());
        put_i32(&mut file, at + 4, i32::try_from(length).unwrap());
    };
    segment(0, typeinfo_at, TYPEINFO_SIZE);
    segment(5, guids_at, 2 * GUID_ENTRY);
    segment(7, names_at, names.len());
    // The type: a dispatch interface of four functions.
    put_u32(&mut file, typeinfo_at, 4);
    put_i32(
        &mut file,
        typeinfo_at + 0x04,
        i32::try_from(block_at).unwrap(),
    );
    put_u32(&mut file, typeinfo_at + 0x18, 4);
    put_i32(
        &mut file,
        typeinfo_at + 0x2C,
        i32::try_from(GUID_ENTRY).unwrap(),
    );
    put_i32(&mut file, typeinfo_at + 0x34, ti_name);
    // Two GUIDs, plainly made up.
    for (index, first) in [0x1111_1111_u32, 0x2222_2222].into_iter().enumerate() {
        let at = guids_at + index * GUID_ENTRY;
        file[at..at + 4].copy_from_slice(&first.to_le_bytes());
        file[at + 4..at + 6].copy_from_slice(&0x1234_u16.to_le_bytes());
        file[at + 6..at + 8].copy_from_slice(&0x5678_u16.to_le_bytes());
        file[at + 8..at + 16].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11]);
        put_i32(&mut file, at + 16, -1);
        put_i32(&mut file, at + 20, -1);
    }
    file[names_at..names_at + names.len()].copy_from_slice(&names);
    file[block_at..].copy_from_slice(&block);
    file
}
