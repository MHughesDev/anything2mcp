//! The stand's own tests. Each acts as the thing under test would: it changes
//! files with the standard library alone, and then asks the stand what is on disk.

use std::error::Error;
use std::fs;
use std::path::Path;

use example_files::control::{EntryKind, Observed, WriteCount, sha256_hex};
use example_files::faults::{Change, Fault, SaveStyle};
use example_files::harness::{FileStand, StandError};
use example_files::reader::JsonStep;
use example_files::seed;

type Outcome = Result<(), Box<dyn Error>>;

/// The published SHA-256 digests of no bytes and of the three bytes `abc`.
const DIGEST_OF_NOTHING: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const DIGEST_OF_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// Saves the way the file pack is designed to: a new file beside the old one,
/// then one replace.
fn replace(stand: &FileStand, name: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let path = stand.path(name)?;
    let temp = path.with_extension("pack-temp");
    fs::write(&temp, bytes)?;
    fs::rename(&temp, &path)?;
    Ok(())
}

/// Sets a fault, and says whether it could be shown here.
fn shown(stand: &FileStand, fault: Fault) -> Result<bool, StandError> {
    match stand.set_fault(fault) {
        Ok(()) => Ok(true),
        Err(e) if e.is_unsupported() => Ok(false),
        Err(e) => Err(e),
    }
}

/// The slice's proof: a file is replaced, and the stand reports its new bytes,
/// its new digest, and that it was replaced once.
#[test]
fn a_replaced_file_is_reported_with_its_new_bytes_and_digest() -> Outcome {
    let stand = FileStand::start()?;
    let before = stand.inspect(seed::STOCK_CSV)?;
    assert_eq!(before.kind, EntryKind::File);

    let old = stand.bytes(seed::STOCK_CSV)?.ok_or("no seed file")?;
    let new = String::from_utf8(old.clone())?.replace("steel\",40,", "steel\",39,");
    assert_ne!(new.as_bytes(), old);
    replace(&stand, seed::STOCK_CSV, new.as_bytes())?;

    let after = stand.inspect(seed::STOCK_CSV)?;
    assert_eq!(
        stand.bytes(seed::STOCK_CSV)?.as_deref(),
        Some(new.as_bytes())
    );
    assert_eq!(after.sha256, Some(sha256_hex(new.as_bytes())));
    assert_ne!(after.sha256, before.sha256);
    assert_eq!(after.size, u64::try_from(new.len())?);
    assert_eq!(
        stand.stored_cells(seed::STOCK_CSV, ',', "item_code", "EX-1002", "on_hand")?,
        ["39"]
    );
    let count = stand.write_count(seed::STOCK_CSV)?;
    assert_eq!(count.content_changes, 1);
    assert_eq!(count.replacements, cfg!(unix).then_some(1));
    if cfg!(unix) {
        assert_ne!(after.identity, before.identity);
    }
    assert!(stand.strays()?.is_empty(), "the temporary file is gone");

    // The digest is the real one: these bytes have a published digest.
    replace(&stand, seed::STOCK_CSV, b"abc")?;
    assert_eq!(
        stand.sha256(seed::STOCK_CSV)?.as_deref(),
        Some(DIGEST_OF_ABC)
    );
    assert_eq!(stand.write_count(seed::STOCK_CSV)?.content_changes, 2);
    Ok(())
}

#[test]
fn one_call_gives_a_folder_holding_every_seed_file() -> Outcome {
    let stand = FileStand::start()?;
    let seeds = seed::files();
    let mut names: Vec<&str> = seeds.iter().map(|f| f.name).collect();
    names.sort_unstable();
    assert_eq!(stand.listing()?, names);
    for file in &seeds {
        assert_eq!(
            stand.bytes(file.name)?.as_deref(),
            Some(file.bytes.as_slice())
        );
    }
    assert!(stand.root().is_absolute());
    assert_eq!(fs::canonicalize(stand.root())?, stand.root());

    // Nested folders, a large file, a name that is not ASCII, a file of no bytes.
    assert_eq!(stand.inspect("archive/2026/11")?.kind, EntryKind::Folder);
    assert!(stand.inspect(seed::LARGE_CSV)?.size > 1_000_000);
    assert_eq!(
        stand.stored_rows(seed::LARGE_CSV, ',')?.len(),
        seed::LARGE_ROWS + 1
    );
    assert!(!seed::NON_ASCII_NAME_CSV.is_ascii());
    let on_disk: Vec<String> = fs::read_dir(stand.path("prices")?)?
        .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    assert!(on_disk.iter().any(|name| name == "价格表-ß.csv"));
    assert_eq!(
        stand.sha256(seed::EMPTY_CSV)?.as_deref(),
        Some(DIGEST_OF_NOTHING)
    );

    // Nothing has been written yet, and nothing is outside its place.
    assert!(stand.observe()?.is_empty());
    assert!(stand.write_counts()?.is_empty());
    assert!(stand.strays()?.is_empty());
    assert!(stand.faults().is_empty());
    assert!(stand.outside_untouched()?);
    assert_eq!(
        stand.inspect("prices/no-such-file.csv")?.kind,
        EntryKind::Absent
    );
    assert!(!stand.exists("no-such-folder/no-such-file.csv")?);
    assert_eq!(stand.bytes("prices")?, None, "a folder has no bytes");
    Ok(())
}

#[test]
fn two_stands_share_nothing() -> Outcome {
    let first = FileStand::start()?;
    let second = FileStand::start()?;
    assert_ne!(first.root(), second.root());
    replace(&first, seed::STOCK_CSV, b"abc")?;
    let untouched = seed::files()
        .into_iter()
        .find(|f| f.name == seed::STOCK_CSV)
        .ok_or("no seed")?;
    assert_eq!(second.bytes(seed::STOCK_CSV)?, Some(untouched.bytes));

    let home = first.root().parent().ok_or("no parent")?.to_path_buf();
    drop(first);
    assert!(!home.exists(), "a stand removes its folder when it ends");
    Ok(())
}

#[test]
fn the_seed_bytes_keep_their_line_endings_and_encoding() -> Outcome {
    let stand = FileStand::start()?;
    let bytes = |name: &str| -> Result<Vec<u8>, Box<dyn Error>> {
        stand.bytes(name)?.ok_or_else(|| name.into())
    };
    {
        let stock: Vec<u8> = bytes(seed::STOCK_CSV)?;
        assert!(!stock.contains(&b'\r') && stock.ends_with(b"\n"));
        assert!(!stock.starts_with(&[0xEF, 0xBB, 0xBF]));

        let prices: Vec<u8> = bytes(seed::PRICE_LIST_CSV)?;
        assert!(prices.starts_with(&[0xEF, 0xBB, 0xBF]), "a byte-order mark");
        let line_feeds = prices.iter().filter(|b| **b == b'\n').count();
        let pairs = prices.windows(2).filter(|w| w == b"\r\n").count();
        assert_eq!((line_feeds, pairs), (4, 4), "every line ending is CRLF");
        assert!(prices.ends_with(b"EUR"), "the last line is not ended");

        let fixed: Vec<u8> = bytes(seed::ORDERS_FIXED_WIDTH)?;
        assert!(fixed.ends_with(b"\r\n"));
        assert!(
            fixed
                .split(|b| *b == b'\n')
                .filter(|l| !l.is_empty())
                .all(|l| l.len() == 35)
        );

        assert!(!bytes(seed::UNITS_JSON)?.contains(&b'\n'));
        assert!(!bytes(seed::TERMS_TEXT)?.ends_with(b"\n"));
        assert!(String::from_utf8(bytes(seed::NOT_UTF8_CSV)?).is_err());
        assert!(String::from_utf8(bytes(seed::STOCK_CSV)?)?.contains('\u{202F}'));
    }
    Ok(())
}

#[test]
fn the_stands_own_reader_reports_the_value_at_a_place() -> Outcome {
    let stand = FileStand::start()?;
    let stock = |key: &str, header: &str| {
        stand.stored_cells(seed::STOCK_CSV, ',', "item_code", key, header)
    };
    assert_eq!(stock("EX-1002", "on_hand")?, ["40"]);
    assert_eq!(stock("EX-1002", "description")?, ["Bracket, steel"]);
    assert_eq!(stock("EX-1002", "note")?, ["Says \"heavy\" on the box"]);
    assert_eq!(stock("EX-1003", "description")?, ["Panel\ntwo lines"]);
    assert_eq!(stock("EX-1003", "on_hand")?, ["0040"], "padding is kept");
    assert_eq!(stock("EX-1004", "on_hand")?, [" 7 "], "spaces are kept");
    assert_eq!(stock("EX-1005", "note")?, ["ends with a space "]);
    assert!(stock("EX-9999", "on_hand")?.is_empty());
    assert!(matches!(
        stock("EX-1002", "no_such_column"),
        Err(StandError::NoSuchColumn { .. })
    ));

    // A byte-order mark is not part of the first header; CRLF is not part of the last.
    assert_eq!(
        stand.stored_cells(
            seed::PRICE_LIST_CSV,
            ',',
            "item_code",
            "EX-1003",
            "unit_price"
        )?,
        ["1,250.00"]
    );
    assert_eq!(
        stand.stored_cells(
            seed::PRICE_LIST_CSV,
            ',',
            "item_code",
            "EX-1004",
            "currency"
        )?,
        ["EUR"]
    );
    assert_eq!(
        stand.stored_cells(
            seed::NON_ASCII_NAME_CSV,
            ';',
            "item_code",
            "EX-1002",
            "preis"
        )?,
        ["10,00"]
    );
    assert_eq!(
        stand.stored_cells(seed::ORDERS_TSV, '\t', "order_id", "A-102", "customer")?,
        [""]
    );
    assert_eq!(
        stand.stored_cells(
            seed::ARCHIVED_STOCK_CSV,
            ',',
            "item_code",
            "EX-1002",
            "on_hand"
        )?,
        ["44"]
    );
    // Two rows with one key are both reported.
    assert_eq!(
        stand.stored_cells(
            seed::STOCK_DUPLICATE_KEY_CSV,
            ',',
            "item_code",
            "EX-1002",
            "on_hand"
        )?,
        ["40", "41"]
    );
    assert!(stand.stored_rows(seed::EMPTY_CSV, ',')?.is_empty());

    let settings = |path: &[JsonStep]| stand.stored_json_values(seed::SETTINGS_JSON, path);
    let warehouse = |code: &str, key: &str| {
        settings(&[
            JsonStep::key("warehouses"),
            JsonStep::element_where("code", code),
            JsonStep::key(key),
        ])
    };
    assert_eq!(warehouse("W2", "cutoff_time")?, ["14:00"]);
    assert_eq!(warehouse("W1", "open")?, ["true"]);
    assert_eq!(warehouse("W3", "open")?, ["null"]);
    assert!(warehouse("W9", "cutoff_time")?.is_empty());
    // Numbers come back as they are spelled, strings with their escapes resolved.
    assert_eq!(settings(&[JsonStep::key("standard_discount")])?, ["2.50"]);
    assert_eq!(settings(&[JsonStep::key("tax_rate")])?, ["1.9e1"]);
    assert_eq!(settings(&[JsonStep::key("rounding")])?, ["-0"]);
    assert_eq!(
        settings(&[JsonStep::key("labels"), JsonStep::key("en")])?,
        ["Café / bar"]
    );
    assert_eq!(
        stand.stored_json_values(
            seed::UNITS_JSON,
            &[JsonStep::key("sizes"), JsonStep::Index(1)]
        )?,
        ["2.0"]
    );
    assert_eq!(
        stand.stored_json_values(seed::DUPLICATE_KEY_JSON, &[JsonStep::key("cutoff_time")])?,
        ["16:30", "17:00"]
    );
    Ok(())
}

#[test]
fn a_file_that_is_not_valid_in_its_format_is_refused_by_the_stands_reader() -> Outcome {
    let stand = FileStand::start()?;
    let unreadable =
        |result: Result<_, StandError>| matches!(result, Err(StandError::Unreadable { .. }));
    assert!(unreadable(stand.stored_rows(seed::BROKEN_CSV, ',')));
    assert!(unreadable(stand.stored_rows(seed::NOT_UTF8_CSV, ',')));
    assert!(unreadable(
        stand
            .stored_json_values(seed::BROKEN_JSON, &[])
            .map(|_| Vec::new())
    ));
    assert!(matches!(
        stand.stored_rows("prices/no-such-file.csv", ','),
        Err(StandError::Absent(_))
    ));
    Ok(())
}

#[test]
fn a_workbook_is_a_package_whose_parts_are_listed_as_they_sit_in_the_file() -> Outcome {
    let stand = FileStand::start()?;
    let names = |file: &str| -> Result<Vec<String>, StandError> {
        Ok(stand
            .package_parts(file)?
            .into_iter()
            .map(|part| part.name)
            .collect())
    };
    let plain = names(seed::PRICE_LIST_WORKBOOK)?;
    assert_eq!(plain[0], "[Content_Types].xml");
    assert!(plain.iter().any(|n| n == "xl/worksheets/sheet1.xml"));
    assert!(plain.iter().any(|n| n == "xl/tables/table1.xml"));
    assert!(!plain.iter().any(|n| n == "xl/vbaProject.bin"));
    assert!(
        names(seed::PRICE_LIST_MACRO_WORKBOOK)?
            .iter()
            .any(|n| n == "xl/vbaProject.bin")
    );
    assert!(
        names(seed::PRICE_LIST_SIGNED_WORKBOOK)?
            .iter()
            .any(|n| n.starts_with("_xmlsignatures/"))
    );
    // The protected file is not a package at all.
    assert!(matches!(
        stand.package_parts(seed::PRICE_LIST_PROTECTED_WORKBOOK),
        Err(StandError::Unreadable { .. })
    ));

    // A change at the place touches one part; every other part sits in the file
    // exactly as it did.
    let before = stand.package_parts(seed::PRICE_LIST_WORKBOOK)?;
    stand.set_fault(Fault::changed(seed::PRICE_LIST_WORKBOOK, Change::ThePlace))?;
    let after = stand.package_parts(seed::PRICE_LIST_WORKBOOK)?;
    let differing: Vec<&str> = before
        .iter()
        .zip(&after)
        .filter(|(b, a)| b.stored_sha256 != a.stored_sha256)
        .map(|(b, _)| b.name.as_str())
        .collect();
    assert_eq!(differing, ["xl/worksheets/sheet1.xml"]);

    // Another layout, and a formula where a value was declared.
    stand.reset()?;
    stand.set_fault(Fault::changed(seed::PRICE_LIST_WORKBOOK, Change::Layout))?;
    let renamed = stand
        .bytes(seed::PRICE_LIST_WORKBOOK)?
        .ok_or("no workbook")?;
    assert!(contains(&renamed, b"<sheet name=\"Price list\""));
    stand.set_fault(Fault::formula_at_declared_value())?;
    let planted = stand
        .bytes(seed::PRICE_LIST_WORKBOOK)?
        .ok_or("no workbook")?;
    assert!(contains(&planted, b"<c r=\"C3\"><f>"));
    Ok(())
}

fn contains(bytes: &[u8], find: &[u8]) -> bool {
    bytes.windows(find.len()).any(|window| window == find)
}

#[test]
fn the_ledger_tells_an_edit_in_place_from_a_replace_where_the_platform_can() -> Outcome {
    let stand = FileStand::start()?;
    let what = |stand: &FileStand| -> Result<Vec<(String, Observed)>, StandError> {
        Ok(stand
            .observe()?
            .into_iter()
            .map(|o| (o.name, o.what))
            .collect())
    };

    fs::write(stand.path(seed::UNITS_JSON)?, b"{}")?;
    assert_eq!(
        what(&stand)?,
        [(
            seed::UNITS_JSON.to_owned(),
            Observed::ContentChanged {
                replaced: cfg!(unix).then_some(false)
            }
        )]
    );
    assert!(what(&stand)?.is_empty(), "a second look sees nothing new");

    replace(&stand, seed::UNITS_JSON, b"[]")?;
    assert_eq!(
        what(&stand)?,
        [(
            seed::UNITS_JSON.to_owned(),
            Observed::ContentChanged {
                replaced: cfg!(unix).then_some(true)
            }
        )]
    );

    // A file left behind, and a file removed.
    fs::write(stand.path("config/left-behind.tmp")?, b"half")?;
    fs::remove_file(stand.path(seed::EMPTY_CSV)?)?;
    assert_eq!(
        what(&stand)?,
        [
            ("config/left-behind.tmp".to_owned(), Observed::Appeared),
            (seed::EMPTY_CSV.to_owned(), Observed::Vanished),
        ]
    );
    assert_eq!(stand.strays()?, ["config/left-behind.tmp"]);

    let count = stand.write_count(seed::UNITS_JSON)?;
    assert_eq!(count.content_changes, 2);
    assert_eq!(count.replacements, cfg!(unix).then_some(1));
    assert_eq!(stand.write_count(seed::EMPTY_CSV)?.disappearances, 1);
    assert_eq!(stand.write_count(seed::STOCK_CSV)?, WriteCount::default());
    assert_eq!(stand.write_counts()?.len(), 3);
    Ok(())
}

#[test]
fn fault_another_writer_changes_the_place_or_elsewhere() -> Outcome {
    let stand = FileStand::start()?;
    let on_hand = |key: &str| stand.stored_cells(seed::STOCK_CSV, ',', "item_code", key, "on_hand");
    let before = stand.inspect(seed::STOCK_CSV)?;

    stand.set_fault(Fault::changed(seed::STOCK_CSV, Change::Elsewhere))?;
    assert_eq!(on_hand("EX-1002")?, ["40"], "the place is as it was");
    assert_eq!(on_hand("EX-1001")?, ["11"]);
    let elsewhere = stand.inspect(seed::STOCK_CSV)?;
    assert_ne!(elsewhere.sha256, before.sha256);
    if cfg!(unix) {
        assert_ne!(elsewhere.identity, before.identity, "saved by a replace");
    }

    stand.set_fault(Fault::ChangedByAnother {
        file: seed::STOCK_CSV.to_owned(),
        change: Change::ThePlace,
        how: SaveStyle::InPlace,
    })?;
    assert_eq!(on_hand("EX-1002")?, ["38"]);
    if cfg!(unix) {
        assert_eq!(
            stand.inspect(seed::STOCK_CSV)?.identity,
            elsewhere.identity,
            "saved in place"
        );
    }

    // The same for JSON and for the delimited file with a byte-order mark.
    let cutoff = [
        JsonStep::key("warehouses"),
        JsonStep::element_where("code", "W2"),
        JsonStep::key("cutoff_time"),
    ];
    stand.set_fault(Fault::changed(seed::SETTINGS_JSON, Change::Elsewhere))?;
    assert_eq!(
        stand.stored_json_values(seed::SETTINGS_JSON, &cutoff)?,
        ["14:00"]
    );
    assert_eq!(
        stand.stored_json_values(seed::SETTINGS_JSON, &[JsonStep::key("schema_version")])?,
        ["4"]
    );
    stand.set_fault(Fault::changed(seed::SETTINGS_JSON, Change::ThePlace))?;
    assert_eq!(
        stand.stored_json_values(seed::SETTINGS_JSON, &cutoff)?,
        ["13:00"]
    );
    stand.set_fault(Fault::changed(seed::PRICE_LIST_CSV, Change::ThePlace))?;
    stand.set_fault(Fault::changed(seed::PRICE_LIST_CSV, Change::Elsewhere))?;
    let price =
        |key: &str| stand.stored_cells(seed::PRICE_LIST_CSV, ',', "item_code", key, "unit_price");
    assert_eq!(
        (price("EX-1002")?, price("EX-1001")?),
        (vec!["11.00".to_owned()], vec!["4.75".to_owned()])
    );

    // Any bytes, and a whole file.
    stand.set_fault(Fault::changed(
        seed::ORDERS_TSV,
        Change::Bytes {
            find: b"submitted".to_vec(),
            replace: b"cancelled".to_vec(),
        },
    ))?;
    assert_eq!(
        stand.stored_cells(seed::ORDERS_TSV, '\t', "order_id", "A-101", "status")?,
        ["cancelled"]
    );
    stand.set_fault(Fault::changed(
        seed::TERMS_TEXT,
        Change::Whole(b"abc".to_vec()),
    ))?;
    assert_eq!(
        stand.sha256(seed::TERMS_TEXT)?.as_deref(),
        Some(DIGEST_OF_ABC)
    );

    // What the stand did is not counted as a write by the thing under test, and
    // it left nothing behind.
    assert!(stand.write_counts()?.is_empty());
    assert!(stand.strays()?.is_empty());
    assert_eq!(stand.faults().len(), 8);

    // A change the stand does not know, and bytes that do not occur.
    assert!(matches!(
        stand.set_fault(Fault::changed(seed::TERMS_TEXT, Change::ThePlace)),
        Err(StandError::NoKnownChange(_))
    ));
    assert!(matches!(
        stand.set_fault(Fault::changed(
            seed::ORDERS_TSV,
            Change::Bytes {
                find: b"no such bytes".to_vec(),
                replace: Vec::new()
            }
        )),
        Err(StandError::BytesNotFound(_))
    ));
    Ok(())
}

#[test]
fn fault_the_layout_changes() -> Outcome {
    let stand = FileStand::start()?;
    stand.set_fault(Fault::changed(seed::STOCK_CSV, Change::Layout))?;
    assert!(matches!(
        stand.stored_cells(seed::STOCK_CSV, ',', "item_code", "EX-1002", "on_hand"),
        Err(StandError::NoSuchColumn { .. })
    ));
    assert_eq!(
        stand.stored_cells(
            seed::STOCK_CSV,
            ',',
            "item_code",
            "EX-1002",
            "quantity_on_hand"
        )?,
        ["40"]
    );

    stand.set_fault(Fault::changed(seed::SETTINGS_JSON, Change::Layout))?;
    let key = |name: &str| {
        stand.stored_json_values(
            seed::SETTINGS_JSON,
            &[
                JsonStep::key("warehouses"),
                JsonStep::Index(1),
                JsonStep::key(name),
            ],
        )
    };
    assert!(key("cutoff_time")?.is_empty());
    assert_eq!(key("cutoff")?, ["14:00"]);

    stand.set_fault(Fault::changed(seed::PRICE_LIST_CSV, Change::Layout))?;
    assert_eq!(
        stand.stored_rows(seed::PRICE_LIST_CSV, ',')?[0],
        ["item_code", "length", "price", "currency"]
    );
    Ok(())
}

#[test]
fn fault_a_file_is_removed_and_a_folder_disappears() -> Outcome {
    let stand = FileStand::start()?;
    stand.set_fault(Fault::FileRemoved {
        file: seed::STOCK_CSV.to_owned(),
    })?;
    let gone = stand.inspect(seed::STOCK_CSV)?;
    assert_eq!(
        (gone.kind, gone.exists(), gone.sha256),
        (EntryKind::Absent, false, None)
    );
    assert_eq!(stand.bytes(seed::STOCK_CSV)?, None);

    stand.set_fault(Fault::FolderRemoved {
        folder: "config".to_owned(),
    })?;
    assert_eq!(stand.inspect("config")?.kind, EntryKind::Absent);
    assert!(!stand.exists(seed::SETTINGS_JSON)?);
    assert!(
        !stand
            .listing()?
            .iter()
            .any(|name| name.starts_with("config/"))
    );
    assert!(!stand.path("config")?.exists());
    Ok(())
}

#[test]
fn fault_a_partial_write_is_left_behind() -> Outcome {
    let stand = FileStand::start()?;
    let whole = stand.bytes(seed::SETTINGS_JSON)?.ok_or("no seed")?;
    stand.set_fault(Fault::PartialWrite {
        beside: seed::SETTINGS_JSON.to_owned(),
        temp_name: ".example-settings.json.tmp-0001".to_owned(),
    })?;
    let left = "config/.example-settings.json.tmp-0001";
    assert_eq!(
        stand.bytes(left)?.as_deref(),
        Some(&whole[..whole.len() / 2])
    );
    assert_eq!(
        stand.bytes(seed::SETTINGS_JSON)?,
        Some(whole),
        "the file is whole"
    );
    assert!(stand.listing()?.iter().any(|name| name == left));
    assert!(stand.strays()?.is_empty(), "the stand knows it made it");
    // It stays until a reset; it is on disk, not a setting.
    stand.clear_faults()?;
    assert!(stand.exists(left)?);
    Ok(())
}

#[test]
fn fault_the_lock_signal_is_present() -> Outcome {
    let stand = FileStand::start()?;
    stand.set_fault(Fault::lock_signal_for(seed::PRICE_LIST_WORKBOOK))?;
    let signal = "prices/price-list.xlsx.lock";
    assert_eq!(stand.inspect(signal)?.kind, EntryKind::File);
    assert!(stand.path(signal)?.exists());
    assert!(stand.strays()?.is_empty());
    assert_eq!(
        stand.faults(),
        [Fault::lock_signal_for(seed::PRICE_LIST_WORKBOOK)]
    );

    stand.clear_faults()?;
    assert!(!stand.exists(signal)?);
    assert!(stand.faults().is_empty());
    Ok(())
}

#[test]
fn fault_a_file_is_read_only() -> Outcome {
    let stand = FileStand::start()?;
    let path = stand.path(seed::STOCK_CSV)?;
    assert!(!stand.inspect(seed::STOCK_CSV)?.read_only);
    stand.set_fault(Fault::ReadOnlyFile {
        file: seed::STOCK_CSV.to_owned(),
    })?;
    assert!(stand.inspect(seed::STOCK_CSV)?.read_only);
    assert!(fs::metadata(&path)?.permissions().readonly());
    // On Windows the mark stops an edit in place, whoever asks. On Unix it does
    // not stop the superuser, so nothing is claimed there.
    if cfg!(windows) {
        assert!(fs::write(&path, b"abc").is_err());
    }

    stand.clear_faults()?;
    assert!(!stand.inspect(seed::STOCK_CSV)?.read_only);
    replace(&stand, seed::STOCK_CSV, b"abc")?;
    assert_eq!(
        stand.sha256(seed::STOCK_CSV)?.as_deref(),
        Some(DIGEST_OF_ABC)
    );
    Ok(())
}

#[test]
fn fault_a_folder_refuses_every_write() -> Outcome {
    let stand = FileStand::start()?;
    let fault = Fault::ReadOnlyFolder {
        folder: "prices".to_owned(),
    };
    assert_eq!(fault.exists_here(), cfg!(unix));
    if !shown(&stand, fault)? {
        // Not on this operating system, or not to the user running the tests.
        assert!(stand.faults().is_empty());
        assert!(!stand.inspect("prices")?.read_only, "nothing is left set");
        return Ok(());
    }
    let before = stand.sha256(seed::STOCK_CSV)?;
    let temp = stand.path("prices/stock.pack-temp")?;
    assert!(fs::write(&temp, b"abc").is_err(), "no new file can be made");
    assert!(fs::remove_file(stand.path(seed::STOCK_CSV)?).is_err());
    assert_eq!(stand.sha256(seed::STOCK_CSV)?, before);
    assert!(stand.strays()?.is_empty());

    stand.clear_faults()?;
    replace(&stand, seed::STOCK_CSV, b"abc")?;
    assert_eq!(
        stand.sha256(seed::STOCK_CSV)?.as_deref(),
        Some(DIGEST_OF_ABC)
    );
    Ok(())
}

#[test]
fn fault_a_file_is_held_open_without_sharing() -> Outcome {
    let stand = FileStand::start()?;
    let fault = Fault::HeldWithoutSharing {
        file: seed::STOCK_CSV.to_owned(),
    };
    let exists_here = fault.exists_here();
    assert_eq!(exists_here, cfg!(windows));
    if !shown(&stand, fault)? {
        assert!(!exists_here, "Windows must be able to show this fault");
        return Ok(());
    }
    let path = stand.path(seed::STOCK_CSV)?;
    let seed_bytes = seed::files()
        .into_iter()
        .find(|f| f.name == seed::STOCK_CSV)
        .ok_or("no seed")?
        .bytes;
    assert!(fs::read(&path).is_err(), "nobody else can open it");
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    // The stand still reports the truth, through the handle that holds the file.
    assert_eq!(stand.bytes(seed::STOCK_CSV)?, Some(seed_bytes));

    stand.clear_faults()?;
    replace(&stand, seed::STOCK_CSV, b"abc")?;
    assert_eq!(
        stand.sha256(seed::STOCK_CSV)?.as_deref(),
        Some(DIGEST_OF_ABC)
    );
    Ok(())
}

#[test]
fn fault_a_link_leads_out_of_the_root() -> Outcome {
    let stand = FileStand::start()?;
    let file_link = "prices/linked-out.csv";
    let folder_link = "linked-folder";
    if !shown(
        &stand,
        Fault::LinkOut {
            at: file_link.to_owned(),
        },
    )? {
        assert_ne!(
            std::env::consts::FAMILY,
            "unix",
            "Unix must be able to show this fault"
        );
        return Ok(());
    }
    stand.set_fault(Fault::FolderLinkOut {
        at: folder_link.to_owned(),
    })?;

    // The stand reports the links as links and never follows them.
    for link in [file_link, folder_link] {
        let truth = stand.inspect(link)?;
        assert_eq!((truth.kind, truth.sha256.as_ref()), (EntryKind::Link, None));
        assert_eq!(stand.bytes(link)?, None);
    }
    let listing = stand.listing()?;
    assert!(listing.iter().any(|name| name == file_link));
    assert!(listing.iter().any(|name| name == folder_link));
    assert!(
        !listing
            .iter()
            .any(|name| name.starts_with("linked-folder/"))
    );
    assert!(stand.strays()?.is_empty());

    // The links are real: a reader that follows them reaches the file outside.
    let outside = stand.outside_folder().join(seed::OUTSIDE_FILE);
    assert_eq!(fs::read(stand.path(file_link)?)?, seed::outside_file());
    assert_eq!(fs::canonicalize(stand.path(file_link)?)?, outside);
    assert_eq!(
        fs::canonicalize(stand.path(folder_link)?.join(seed::OUTSIDE_FILE))?,
        outside
    );
    assert!(stand.outside_untouched()?);

    // And the stand notices a write that went through one.
    fs::write(stand.path(file_link)?, b"abc")?;
    assert!(!stand.outside_untouched()?);

    stand.reset()?;
    assert!(stand.outside_untouched()?);
    assert!(!stand.exists(file_link)? && !stand.exists(folder_link)?);
    Ok(())
}

#[test]
fn fault_a_file_has_two_links() -> Outcome {
    let stand = FileStand::start()?;
    assert_eq!(
        stand.inspect(seed::STOCK_CSV)?.links,
        cfg!(unix).then_some(1)
    );
    stand.set_fault(Fault::SecondLink {
        file: seed::STOCK_CSV.to_owned(),
        at: "prices/stock-second-name.csv".to_owned(),
    })?;
    let first = stand.inspect(seed::STOCK_CSV)?;
    let second = stand.inspect("prices/stock-second-name.csv")?;
    assert_eq!(first.links, cfg!(unix).then_some(2));
    assert_eq!(
        second.kind,
        EntryKind::File,
        "a second name is not a link to follow"
    );
    assert_eq!(first.sha256, second.sha256);
    assert_eq!(first.identity, second.identity);

    // They are one file: a write in place through one name shows under the other.
    fs::write(stand.path("prices/stock-second-name.csv")?, b"abc")?;
    assert_eq!(
        stand.sha256(seed::STOCK_CSV)?.as_deref(),
        Some(DIGEST_OF_ABC)
    );
    Ok(())
}

#[test]
fn every_text_that_tries_to_leave_the_root_names_the_file_outside() -> Outcome {
    let stand = FileStand::start()?;
    let outside = stand.outside_folder().join(seed::OUTSIDE_FILE);
    assert!(!outside.starts_with(stand.root()));
    let attempts = stand.escape_attempts();
    assert_eq!(attempts.len(), 4);
    for attempt in &attempts {
        // The stand's own names never leave the root.
        assert!(
            matches!(stand.path(&attempt.text), Err(StandError::BadName(_))),
            "{}",
            attempt.how
        );
    }
    assert!(Path::new(&attempts[3].text).is_absolute());
    assert_eq!(Path::new(&attempts[3].text), outside);
    // Joined to the root by a careless opener, the parent steps do reach it.
    // Windows does not resolve parent steps in the long form of a path.
    if cfg!(unix) {
        for attempt in &attempts[..2] {
            assert_eq!(fs::canonicalize(stand.root().join(&attempt.text))?, outside);
        }
    }
    for bad in ["", "/", "a//b", "./a", "a/..", "a\\b", "c:a"] {
        assert!(
            matches!(stand.path(bad), Err(StandError::BadName(_))),
            "{bad}"
        );
    }
    assert!(stand.outside_untouched()?);
    Ok(())
}

#[test]
fn reset_restores_the_seed_and_ends_every_fault() -> Outcome {
    let stand = FileStand::start()?;
    replace(&stand, seed::STOCK_CSV, b"abc")?;
    fs::write(stand.path("prices/stray.tmp")?, b"half")?;
    stand.set_fault(Fault::changed(seed::SETTINGS_JSON, Change::Layout))?;
    stand.set_fault(Fault::lock_signal_for(seed::STOCK_CSV))?;
    stand.set_fault(Fault::ReadOnlyFile {
        file: seed::UNITS_JSON.to_owned(),
    })?;
    stand.set_fault(Fault::FolderRemoved {
        folder: "archive".to_owned(),
    })?;
    let _ = shown(
        &stand,
        Fault::ReadOnlyFolder {
            folder: "exports".to_owned(),
        },
    )?;
    let _ = shown(
        &stand,
        Fault::HeldWithoutSharing {
            file: seed::TERMS_TEXT.to_owned(),
        },
    )?;
    assert!(stand.faults().len() >= 4);
    assert_eq!(stand.write_count(seed::STOCK_CSV)?.content_changes, 1);

    stand.reset()?;
    let seeds = seed::files();
    let mut names: Vec<&str> = seeds.iter().map(|f| f.name).collect();
    names.sort_unstable();
    assert_eq!(stand.listing()?, names);
    for file in &seeds {
        assert_eq!(
            stand.bytes(file.name)?.as_deref(),
            Some(file.bytes.as_slice())
        );
        assert!(!stand.inspect(file.name)?.read_only);
    }
    assert!(stand.faults().is_empty());
    assert!(stand.write_counts()?.is_empty());
    assert!(stand.strays()?.is_empty());
    Ok(())
}
