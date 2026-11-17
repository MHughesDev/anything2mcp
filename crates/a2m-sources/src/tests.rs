//! The crate's invariants, each a test, over inputs built from code: the
//! file stand's seed files and workbooks built here part by part.

use std::sync::Arc;

use a2m_core::{
    Date, Decimal, Digest, Environment, EnvironmentIndex, EnvironmentKind, EnvironmentPlan,
    ExprIndex, FieldFormat, FieldTypeIndex, FieldTypePlan, PLAN_FORMAT, PackPayload, Plan,
    PlanHeader, Rounding, Sensitivity, SourcePlan, StepArg, TargetName, TargetPlan, Value,
    ValueType, Values, Version,
};
use a2m_expr::{DecimalMark, FieldRule, LibraryVersion, TypeEnv, Whitespace, compile};
use example_files::seed;

use crate::testing::{LIMITS, Packed, package};
use crate::*;

// ---------------------------------------------------------------- helpers

fn seed_file(name: &str) -> Vec<u8> {
    seed::files()
        .into_iter()
        .find(|file| file.name == name)
        .map(|file| file.bytes)
        .unwrap()
}

fn input(bytes: &[u8]) -> SourceInput<'_> {
    SourceInput {
        bytes,
        limits: LIMITS,
        options: ReaderOptions::default(),
    }
}

/// A plan that holds only field types and expressions.
struct Target {
    plan: Plan,
}

impl Target {
    fn new() -> Self {
        Self {
            plan: Plan {
                header: PlanHeader {
                    format: PLAN_FORMAT,
                    minimum_engine: Version::new(0, 0, 1),
                    compiler: Version::new(0, 0, 1),
                    target: TargetName::new("example").unwrap(),
                    version: Version::new(1, 0, 0),
                    sources: Digest::of(b"sources/example.yaml"),
                },
                target: TargetPlan {
                    title: "Example".to_owned(),
                    modules: vec![],
                    default_environment: EnvironmentIndex::new(0),
                    locale: None,
                    timezone: None,
                    capture: None,
                    exclusive_session: false,
                    drift_risk: None,
                    authorization: None,
                },
                surfaces: vec![],
                environments: vec![EnvironmentPlan {
                    name: Environment::new("production").unwrap(),
                    kind: EnvironmentKind::Production,
                    addresses: vec![],
                    test_records: vec![],
                    approve_writes: None,
                }],
                auth: vec![],
                field_types: vec![],
                expressions: vec![],
                operations: vec![],
                pipelines: vec![],
                tools: vec![],
                packs: vec![],
                tests: vec![],
            },
        }
    }

    fn expression(&mut self, source: &str, result: &ValueType) -> ExprIndex {
        let mut env = TypeEnv::new(LibraryVersion::CURRENT);
        env.declare_unknown("row").unwrap();
        env.declare_unknown("source").unwrap();
        let compiled = compile(source, &env, Some(result)).unwrap();
        self.plan.expressions.push(compiled.to_plan_expr().unwrap());
        ExprIndex::new(u32::try_from(self.plan.expressions.len() - 1).unwrap())
    }

    fn field_type(&mut self, name: &str, rule: &FieldRule) -> FieldTypeIndex {
        let normalizer = rule.normalizer().unwrap().to_plan_expr().unwrap();
        self.plan.expressions.push(normalizer);
        let normalizer = ExprIndex::new(u32::try_from(self.plan.expressions.len() - 1).unwrap());
        self.plan.field_types.push(FieldTypePlan {
            name: name.to_owned(),
            base: rule.base(),
            ty: rule.value_type(),
            places: None,
            rounding: None,
            step: None,
            unit: None,
            display_unit: None,
            currency: None,
            zone: None,
            codes: vec![],
            sensitivity: Sensitivity::default(),
            untrusted: false,
            // The readers type a cell by the normalizer and never read the
            // format, so these plans do not state a true one.
            format: FieldFormat::Choice,
            normalizer: Some(normalizer),
        });
        FieldTypeIndex::new(u32::try_from(self.plan.field_types.len() - 1).unwrap())
    }

    fn text(&mut self) -> FieldTypeIndex {
        self.field_type(
            "text",
            &FieldRule::Text {
                case_sensitive: true,
                whitespace: Whitespace::Exact,
            },
        )
    }

    fn integer(&mut self) -> FieldTypeIndex {
        self.field_type(
            "integer",
            &FieldRule::Integer {
                mark: DecimalMark::Point,
            },
        )
    }

    fn decimal(&mut self, places: u32) -> FieldTypeIndex {
        self.field_type(
            "decimal",
            &FieldRule::Decimal {
                places,
                rounding: Rounding::HalfEven,
                mark: DecimalMark::Point,
            },
        )
    }

    fn date(&mut self) -> FieldTypeIndex {
        self.field_type(
            "date",
            &FieldRule::Date {
                format: "YYYY-MM-DD".to_owned(),
            },
        )
    }

    fn load(
        &self,
        reader: &str,
        spec: &SourceSpec,
        mapping: Vec<StepArg>,
        rows: Option<ExprIndex>,
    ) -> LoadedSource {
        self.try_load(reader, spec, mapping, rows).unwrap()
    }

    fn try_load(
        &self,
        reader: &str,
        spec: &SourceSpec,
        mapping: Vec<StepArg>,
        rows: Option<ExprIndex>,
    ) -> Result<LoadedSource, a2m_core::Failure> {
        LoadedSource::load(
            &self.plan,
            &SourcePlan {
                template: "example_stock".to_owned(),
                reader: reader.to_owned(),
                mapping,
                rows,
                payload: PackPayload::encode(spec).unwrap(),
            },
        )
    }
}

fn column(name: &str, header: &str, field_type: FieldTypeIndex, required: bool) -> RowField {
    RowField {
        name: name.to_owned(),
        field_type,
        required,
        column: Some(header.to_owned()),
    }
}

/// A definition whose one template version is the layout of `template`.
fn spec(
    reader: &str,
    template: &SourceInput<'_>,
    locate: Locate,
    fields: Vec<RowField>,
) -> SourceSpec {
    let described = Sources::builtin().describe(reader, template).unwrap();
    let columns = fields
        .iter()
        .filter_map(|field| field.column.clone())
        .collect();
    SourceSpec {
        versions: vec![TemplateVersion {
            version: "1".to_owned(),
            fingerprint: described.fingerprint,
        }],
        locate,
        columns,
        fields,
        key: vec![],
        missing: vec![],
        hidden: HiddenRule::Reject,
        options: template.options,
    }
}

fn decimal(text: &str) -> Value {
    Value::Decimal(Decimal::parse(text).unwrap())
}

fn field<'a>(records: &'a ValidatedRecords, row: usize, name: &str) -> &'a Value {
    records.rows[row].fields.get(name).unwrap()
}

/// The parts of a small workbook, for tests that need a layout the stand's
/// workbook does not have.
struct Book {
    date1904: bool,
    styles: &'static str,
    shared: &'static [&'static str],
    sheet: String,
    packed: Packed,
}

const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// Two cell styles: 0 is plain, 1 is a date.
const STYLES: &str = "<cellXfs count=\"2\"><xf numFmtId=\"0\"/><xf numFmtId=\"14\"/></cellXfs>";

impl Book {
    fn new(sheet: &str) -> Self {
        Self {
            date1904: false,
            styles: STYLES,
            shared: &[],
            sheet: sheet.to_owned(),
            packed: Packed::Stored,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let types =
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>"
                .to_owned();
        let properties = if self.date1904 {
            "<workbookPr date1904=\"1\"/>"
        } else {
            ""
        };
        let book = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<workbook xmlns=\"{NS}\" xmlns:r=\"{REL}\">{properties}\
             <sheets><sheet name=\"Stock\" sheetId=\"1\" r:id=\"rId1\"/></sheets>\
             <definedNames><definedName name=\"stock_rows\">Stock!$A$1:$C$3</definedName>\
             <definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"0\" hidden=\"1\">Stock!$A$1:$C$9</definedName></definedNames></workbook>"
        );
        let relations = format!(
            "<Relationships xmlns=\"{PACKAGE_REL}\">\
             <Relationship Id=\"rId1\" Type=\"{REL}/worksheet\" Target=\"worksheets/sheet1.xml\"/>\
             <Relationship Id=\"rId2\" Type=\"{REL}/styles\" Target=\"styles.xml\"/>\
             <Relationship Id=\"rId3\" Type=\"{REL}/sharedStrings\" Target=\"/xl/sharedStrings.xml\"/>\
             <Relationship Id=\"rId4\" Type=\"{REL}/externalLink\" Target=\"https://files.example.test/other.xlsx\" TargetMode=\"External\"/>\
             </Relationships>"
        );
        let styles = format!("<styleSheet xmlns=\"{NS}\">{}</styleSheet>", self.styles);
        let mut strings = format!("<sst xmlns=\"{NS}\">");
        for text in self.shared {
            strings.push_str(text);
        }
        strings.push_str("</sst>");
        let sheet = format!("<worksheet xmlns=\"{NS}\">{}</worksheet>", self.sheet);
        package(&[
            ("[Content_Types].xml", types.as_bytes(), self.packed),
            ("xl/workbook.xml", book.as_bytes(), self.packed),
            (
                "xl/_rels/workbook.xml.rels",
                relations.as_bytes(),
                self.packed,
            ),
            ("xl/styles.xml", styles.as_bytes(), self.packed),
            ("xl/sharedStrings.xml", strings.as_bytes(), self.packed),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes(), self.packed),
        ])
    }
}

/// A sheet of three columns, code, qty, and day, with the rows given.
fn stock_sheet(rows: &str) -> String {
    format!(
        "<sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>code</t></is></c>\
         <c r=\"B1\" t=\"inlineStr\"><is><t>qty</t></is></c>\
         <c r=\"C1\" t=\"inlineStr\"><is><t>day</t></is></c></row>{rows}</sheetData>"
    )
}

fn stock_row(row: u32, code: &str, qty: &str, day: &str) -> String {
    format!(
        "<row r=\"{row}\"><c r=\"A{row}\" t=\"inlineStr\"><is><t>{code}</t></is></c>\
         <c r=\"B{row}\"><v>{qty}</v></c><c r=\"C{row}\" s=\"1\"><v>{day}</v></c></row>"
    )
}

fn stock_fields(target: &mut Target) -> Vec<RowField> {
    vec![
        column("code", "code", target.text(), true),
        column("qty", "qty", target.decimal(20), true),
        column("day", "day", target.date(), false),
    ]
}

// ------------------------------------------------- bytes in, records out

#[test]
fn bytes_go_in_and_validated_records_come_out() {
    let bytes = seed_file(seed::STOCK_CSV);
    let mut target = Target::new();
    let text = target.text();
    let integer = target.integer();
    let spec = SourceSpec {
        key: vec!["item_code".to_owned()],
        ..spec(
            "csv",
            &input(&bytes),
            Locate::Document,
            vec![
                column("item_code", "item_code", text, true),
                column("description", "description", text, true),
                column("on_hand", "on_hand", integer, true),
                column("note", "note", text, false),
            ],
        )
    };
    let def = target.load("csv", &spec, vec![], None);
    let records = Sources::builtin().read(&def, &bytes, &LIMITS).unwrap();

    // The stand's own reader, which shares no code with this crate, says
    // what the file holds.
    let stored = example_files::reader::delimited_rows(&bytes, ',').unwrap();
    assert_eq!(records.rows.len(), stored.len() - 1);
    for (row, stored_row) in records.rows.iter().zip(&stored[1..]) {
        assert_eq!(
            row.fields.get("item_code"),
            Some(&Value::from(stored_row[0].as_str()))
        );
        assert_eq!(
            row.fields.get("description"),
            Some(&Value::from(stored_row[1].as_str()))
        );
        let on_hand: i64 = stored_row[2].trim().parse().unwrap();
        assert_eq!(row.fields.get("on_hand"), Some(&Value::Int(on_hand)));
    }
    assert_eq!(records.template, "example_stock");
    assert_eq!(records.version.as_deref(), Some("1"));
    assert_eq!(
        records.fingerprint,
        Some(spec.versions[0].fingerprint.digest)
    );
    // A number padded with zeros is 40 because the field type is a whole
    // number; an empty field in quotes is no value; a field that ends in a
    // space keeps it; text aimed at an agent is text.
    assert_eq!(field(&records, 2, "on_hand"), &Value::Int(40));
    assert_eq!(field(&records, 2, "note"), &Value::Null);
    assert_eq!(
        field(&records, 4, "note"),
        &Value::from("ends with a space ")
    );
    assert_eq!(records.rows[2].origin, CellRef::new(0, 4, 1));
    assert_eq!(records.rows[2].index, 2);
}

#[test]
fn a_padded_number_stays_text_unless_a_field_type_says_otherwise() {
    let bytes = seed_file(seed::STOCK_CSV);
    let mut target = Target::new();
    let text = target.text();
    let fields = ["item_code", "description", "on_hand", "note"]
        .into_iter()
        .map(|name| column(name, name, text, false))
        .collect();
    let spec = spec("csv", &input(&bytes), Locate::Document, fields);
    let def = target.load("csv", &spec, vec![], None);
    let records = Sources::builtin().read(&def, &bytes, &LIMITS).unwrap();
    assert_eq!(field(&records, 2, "on_hand"), &Value::from("0040"));
    assert_eq!(field(&records, 3, "on_hand"), &Value::from(" 7 "));
}

#[test]
fn the_other_seed_files_read_or_stop_by_name() {
    let sources = Sources::builtin();
    // A byte-order mark, CRLF, a last line not ended, a quoted number.
    let parsed = sources
        .parse("csv", &input(&seed_file(seed::PRICE_LIST_CSV)))
        .unwrap();
    assert_eq!(
        parsed.structure.sheets[0].headers,
        ["item_code", "length", "unit_price", "currency"]
    );
    assert_eq!(parsed.parts[0].rows, 5);
    assert_eq!(
        parsed.parts[0].cell(4, 3).unwrap().cell.raw,
        RawCell::Text("1,250.00".to_owned())
    );
    // A semicolon, declared; and content that is not ASCII.
    let bytes = seed_file(seed::NON_ASCII_NAME_CSV);
    let mut semicolons = input(&bytes);
    semicolons.options.delimiter = Some(';');
    let parsed = sources.parse("csv", &semicolons).unwrap();
    assert_eq!(
        parsed.structure.sheets[0].headers,
        ["item_code", "beschreibung", "preis"]
    );
    // A tab.
    let bytes = seed_file(seed::ORDERS_TSV);
    let mut tabs = input(&bytes);
    tabs.options.delimiter = Some('\t');
    assert!(sources.parse("csv", &tabs).unwrap().parts[0].rows > 1);

    let code = |reader: &str, name: &str| {
        sources
            .parse(reader, &input(&seed_file(name)))
            .unwrap_err()
            .code()
            .clone()
    };
    assert_eq!(code("csv", seed::BROKEN_CSV), SOURCE_MALFORMED);
    assert_eq!(code("csv", seed::NOT_UTF8_CSV), SOURCE_ENCODING_UNSUPPORTED);
    assert_eq!(code("json", seed::BROKEN_JSON), SOURCE_MALFORMED);
    assert_eq!(code("json", seed::DUPLICATE_KEY_JSON), SOURCE_MALFORMED);
    assert_eq!(
        code("xlsx", seed::PRICE_LIST_PROTECTED_WORKBOOK),
        SOURCE_PROTECTED
    );
    assert_eq!(code("yaml", seed::SETTINGS_YAML), SOURCE_READER_UNKNOWN);
}

// ----------------------------------------------------------- exact digits

#[test]
fn a_decimal_keeps_its_exact_digits_through_every_reader() {
    // Neither number survives a float: the first has more digits than one
    // holds, and the second is the float nearest to three tenths.
    let numbers = ["1234567890.0123456789", "0.30000000000000004"];
    let mut target = Target::new();
    let fields = vec![column("amount", "amount", target.decimal(20), true)];
    let sources = Sources::builtin();
    let check = |records: &ValidatedRecords| {
        for (row, number) in numbers.iter().enumerate() {
            assert_eq!(field(records, row, "amount"), &decimal(number));
        }
    };

    let csv = format!("amount\n{}\n{}\n", numbers[0], numbers[1]);
    let def = target.load(
        "csv",
        &spec(
            "csv",
            &input(csv.as_bytes()),
            Locate::Document,
            fields.clone(),
        ),
        vec![],
        None,
    );
    check(&sources.read(&def, csv.as_bytes(), &LIMITS).unwrap());

    let json = format!(
        "[{{\"amount\": {}}}, {{\"amount\": {}}}]",
        numbers[0], numbers[1]
    );
    let def = target.load(
        "json",
        &spec(
            "json",
            &input(json.as_bytes()),
            Locate::Document,
            fields.clone(),
        ),
        vec![],
        None,
    );
    check(&sources.read(&def, json.as_bytes(), &LIMITS).unwrap());

    let sheet = format!(
        "<sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>amount</t></is></c></row>\
         <row r=\"2\"><c r=\"A2\"><v>{}</v></c></row><row r=\"3\"><c r=\"A3\"><v>{}</v></c></row></sheetData>",
        numbers[0], numbers[1]
    );
    let book = Book::new(&sheet).bytes();
    let def = target.load(
        "xlsx",
        &spec(
            "xlsx",
            &input(&book),
            Locate::Sheet("Stock".to_owned()),
            fields,
        ),
        vec![],
        None,
    );
    check(&sources.read(&def, &book, &LIMITS).unwrap());
}

#[test]
fn the_crate_parses_no_float_and_opens_no_file() {
    // The words are put together here so that this file does not hold them.
    let float_types = [format!("f{}", 64), format!("f{}", 32)];
    let file_words = [
        format!("std::{}", "fs"),
        format!("{}Buf", "Path"),
        format!("File::{}", "open"),
    ];
    let modules = [
        include_str!("lib.rs"),
        include_str!("csv.rs"),
        include_str!("definition.rs"),
        include_str!("error.rs"),
        include_str!("fingerprint.rs"),
        include_str!("json.rs"),
        include_str!("locate.rs"),
        include_str!("model.rs"),
        include_str!("number.rs"),
        include_str!("package.rs"),
        include_str!("read.rs"),
        include_str!("typing.rs"),
        include_str!("xlsx.rs"),
        include_str!("xml.rs"),
    ];
    for module in modules {
        for word in float_types.iter().chain(&file_words) {
            assert!(!module.contains(word.as_str()), "a module holds `{word}`");
        }
    }
}

// --------------------------------------------------------------- workbooks

#[test]
fn a_date_serial_is_read_by_the_workbooks_own_date_system() {
    let mut target = Target::new();
    let fields = stock_fields(&mut target);
    let sources = Sources::builtin();
    let sheet = stock_sheet(&stock_row(2, "EX-1001", "12", "46343"));

    let from_1900 = Book::new(&sheet).bytes();
    let def = target.load(
        "xlsx",
        &spec(
            "xlsx",
            &input(&from_1900),
            Locate::Sheet("Stock".to_owned()),
            fields.clone(),
        ),
        vec![],
        None,
    );
    let records = sources.read(&def, &from_1900, &LIMITS).unwrap();
    assert_eq!(
        field(&records, 0, "day"),
        &Value::Date(Date::new(2026, 11, 17).unwrap())
    );

    let from_1904 = Book {
        date1904: true,
        ..Book::new(&sheet)
    }
    .bytes();
    // A workbook in the other date system is another template: the same
    // serial would be another day, so it is never read as the first.
    let rejected = sources.read(&def, &from_1904, &LIMITS).unwrap_err();
    assert_eq!(rejected.failure.code(), &SOURCE_TEMPLATE_UNKNOWN);
    assert!(rejected.problems[0].message.contains("date system"));

    let def = target.load(
        "xlsx",
        &spec(
            "xlsx",
            &input(&from_1904),
            Locate::Sheet("Stock".to_owned()),
            fields,
        ),
        vec![],
        None,
    );
    let records = sources.read(&def, &from_1904, &LIMITS).unwrap();
    assert_eq!(
        field(&records, 0, "day"),
        &Value::Date(Date::new(2030, 11, 18).unwrap())
    );
    // The cell is a date serial because its number format is a date.
    let parsed = sources.parse("xlsx", &input(&from_1904)).unwrap();
    assert_eq!(
        parsed.parts[0].cell(2, 3).unwrap().cell.raw,
        RawCell::DateSerial("46343".to_owned())
    );
    assert_eq!(
        parsed.parts[0].cell(2, 3).unwrap().cell.format.as_deref(),
        Some("mm-dd-yy")
    );
    assert_eq!(
        parsed.parts[0].cell(2, 2).unwrap().cell.raw,
        RawCell::Number("12".to_owned())
    );
}

#[test]
fn the_stands_workbook_is_described_as_the_stand_built_it() {
    let bytes = seed_file(seed::PRICE_LIST_WORKBOOK);
    let described = Sources::builtin().describe("xlsx", &input(&bytes)).unwrap();
    let structure = &described.structure;
    assert_eq!(structure.date_system, Some(a2m_expr::DateSystem::From1900));
    assert_eq!(
        structure
            .sheets
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["Prices", "Notes"]
    );
    let headers = ["Item code", "Length", "Unit price", "On hand", "Line value"];
    assert_eq!(structure.sheets[0].headers, headers);
    assert_eq!(structure.sheets[0].tables[0].name, "price_table");
    assert_eq!(structure.sheets[0].tables[0].range.a1(), "A1:E4");
    assert_eq!(structure.sheets[0].tables[0].headers, headers);
    assert_eq!(structure.sheets[1].merged[0].a1(), "B2:C2");
    assert_eq!(structure.names[0].name, "standard_discount");
    assert_eq!(structure.names[0].target, "Notes!$B$1");
    assert!(!structure.macros && !structure.signed);
    assert_eq!(described.parts[0].rows, 4);
    assert_eq!(described.parts[0].cells, 20);
    assert!(described.fingerprint.is_sound());
    assert_eq!(described.fingerprint, structure.fingerprint());
    // Nothing of a value is in the structure.
    let shown = format!("{structure:?}");
    assert!(!shown.contains("EX-1001") && !shown.contains("46343"));

    // A macro part is never opened; its presence is part of the layout.
    let with_macro = Sources::builtin()
        .describe("xlsx", &input(&seed_file(seed::PRICE_LIST_MACRO_WORKBOOK)))
        .unwrap();
    assert!(with_macro.structure.macros);
    assert_ne!(with_macro.fingerprint.digest, described.fingerprint.digest);
    let signed = Sources::builtin()
        .describe("xlsx", &input(&seed_file(seed::PRICE_LIST_SIGNED_WORKBOOK)))
        .unwrap();
    assert!(signed.structure.signed);
}

#[test]
fn a_table_is_found_by_name_and_its_formulas_give_their_kept_results() {
    let bytes = seed_file(seed::PRICE_LIST_WORKBOOK);
    let mut target = Target::new();
    let text = target.text();
    let places = target.decimal(2);
    let inches = target.field_type(
        "length",
        &FieldRule::Decimal {
            places: 4,
            rounding: Rounding::HalfEven,
            mark: DecimalMark::Point,
        },
    );
    let integer = target.integer();
    let length = target.expression("to_inches(row[\"Length\"])", &ValueType::Decimal);
    let filter = target.expression("row[\"Item code\"] != \"EX-1002\"", &ValueType::Bool);
    let mut spec = spec(
        "xlsx",
        &input(&bytes),
        Locate::Named("price_table".to_owned()),
        vec![
            column("item_code", "Item code", text, true),
            RowField {
                name: "length_in".to_owned(),
                field_type: inches,
                required: true,
                column: None,
            },
            column("unit_price", "Unit price", places, true),
            column("on_hand", "On hand", integer, true),
            column("line_value", "Line value", places, true),
        ],
    );
    spec.columns.insert(1, "Length".to_owned());
    let mapping = vec![StepArg {
        name: "length_in".to_owned(),
        value: length,
    }];
    let def = target.load("xlsx", &spec, mapping.clone(), None);
    let records = Sources::builtin().read(&def, &bytes, &LIMITS).unwrap();
    assert_eq!(records.rows.len(), 3);
    assert_eq!(field(&records, 0, "item_code"), &Value::from("EX-1001"));
    assert_eq!(field(&records, 0, "length_in"), &decimal("150"));
    assert_eq!(field(&records, 2, "length_in"), &decimal("9.5"));
    assert_eq!(field(&records, 0, "unit_price"), &decimal("4.5"));
    // A number the workbook spells `0040` is the number forty.
    assert_eq!(field(&records, 2, "on_hand"), &Value::Int(40));
    // A formula is never calculated: its value is the result the workbook kept.
    assert_eq!(field(&records, 0, "line_value"), &decimal("54"));
    assert_eq!(field(&records, 2, "line_value"), &decimal("50000"));

    // The row filter says which rows are records; the count of the others is kept.
    let def = target.load("xlsx", &spec, mapping.clone(), Some(filter));
    let records = Sources::builtin().read(&def, &bytes, &LIMITS).unwrap();
    assert_eq!(records.rows.len(), 2);
    assert_eq!(records.filtered_out, 1);
    assert_eq!(field(&records, 1, "item_code"), &Value::from("EX-1003"));

    // The same workbook with a formula where a value was: still read, as
    // the kept result.
    let with_formula = seed::workbook(&seed::Workbook {
        formula_at_declared_value: true,
        ..seed::Workbook::default()
    });
    let def = target.load("xlsx", &spec, mapping, None);
    let records = Sources::builtin()
        .read(&def, &with_formula, &LIMITS)
        .unwrap();
    assert_eq!(field(&records, 1, "unit_price"), &decimal("9"));
}

#[test]
fn a_formula_with_no_kept_result_is_a_problem_not_an_empty_cell() {
    let mut target = Target::new();
    let fields = stock_fields(&mut target);
    let template = Book::new(&stock_sheet(&stock_row(2, "EX-1001", "12", "46343"))).bytes();
    let def = target.load(
        "xlsx",
        &spec(
            "xlsx",
            &input(&template),
            Locate::Named("stock_rows".to_owned()),
            fields,
        ),
        vec![],
        None,
    );
    let rows = format!(
        "{}<row r=\"3\"><c r=\"A3\" t=\"inlineStr\"><is><t>EX-1002</t></is></c><c r=\"B3\"><f>B2*2</f></c></row>",
        stock_row(2, "EX-1001", "12", "46343")
    );
    let bytes = Book::new(&stock_sheet(&rows)).bytes();
    let rejected = Sources::builtin().read(&def, &bytes, &LIMITS).unwrap_err();
    assert_eq!(rejected.problems.len(), 1);
    assert_eq!(rejected.problems[0].code, FORMULA_WITHOUT_VALUE);
    assert_eq!(rejected.problems[0].at, Some(CellRef::new(0, 3, 2)));
    assert_eq!(rejected.problems[0].field.as_deref(), Some("qty"));
}

#[test]
fn merged_and_hidden_cells_and_an_empty_row_inside_the_data_are_problems() {
    let mut target = Target::new();
    let fields = stock_fields(&mut target);
    let good = format!(
        "{}{}",
        stock_row(2, "EX-1001", "12", "46343"),
        stock_row(3, "EX-1002", "7", "46344")
    );
    let template = Book::new(&stock_sheet(&good)).bytes();
    let spec = spec(
        "xlsx",
        &input(&template),
        Locate::Sheet("Stock".to_owned()),
        fields,
    );
    let def = target.load("xlsx", &spec, vec![], None);
    let sources = Sources::builtin();
    assert_eq!(
        sources.read(&def, &template, &LIMITS).unwrap().rows.len(),
        2
    );

    // A hidden row: recorded on its cells, and refused unless the
    // definition says hidden cells are read.
    let hidden_row = good.replace("<row r=\"3\">", "<row r=\"3\" hidden=\"1\">");
    let bytes = Book::new(&stock_sheet(&hidden_row)).bytes();
    let parsed = sources.parse("xlsx", &input(&bytes)).unwrap();
    assert!(parsed.parts[0].cell(3, 1).unwrap().cell.hidden);
    assert!(!parsed.parts[0].cell(2, 1).unwrap().cell.hidden);
    let rejected = sources.read(&def, &bytes, &LIMITS).unwrap_err();
    assert_eq!(rejected.problems.len(), 1);
    assert_eq!(rejected.problems[0].at.unwrap().row, 3);
    assert!(rejected.problems[0].message.contains("hidden"));
    let including = target.load(
        "xlsx",
        &SourceSpec {
            hidden: HiddenRule::Include,
            ..spec.clone()
        },
        vec![],
        None,
    );
    assert_eq!(
        sources
            .read(&including, &bytes, &LIMITS)
            .unwrap()
            .rows
            .len(),
        2
    );

    // A hidden column.
    let hidden_column = format!(
        "<cols><col min=\"2\" max=\"2\" hidden=\"1\"/></cols>{}",
        stock_sheet(&good)
    );
    let bytes = Book::new(&hidden_column).bytes();
    let rejected = sources.read(&def, &bytes, &LIMITS).unwrap_err();
    assert_eq!(rejected.problems.len(), 1);
    assert_eq!(rejected.problems[0].field.as_deref(), Some("qty"));

    // A merged range inside the data changes the layout, so the file is an
    // unknown template; a definition that knows the layout still refuses to
    // read rows through it.
    let merged = format!(
        "{}<mergeCells><mergeCell ref=\"A2:A3\"/></mergeCells>",
        stock_sheet(&good)
    );
    let bytes = Book::new(&merged).bytes();
    assert_eq!(
        sources
            .read(&def, &bytes, &LIMITS)
            .unwrap_err()
            .failure
            .code(),
        &SOURCE_TEMPLATE_UNKNOWN
    );
    let parsed = sources.parse("xlsx", &input(&bytes)).unwrap();
    assert_eq!(
        parsed.parts[0]
            .cell(2, 1)
            .unwrap()
            .cell
            .merged
            .unwrap()
            .a1(),
        "A2:A3"
    );
    let knowing = SourceSpec {
        versions: vec![TemplateVersion {
            version: "merged".to_owned(),
            fingerprint: parsed.structure.fingerprint(),
        }],
        ..spec.clone()
    };
    let rejected = sources
        .read(
            &target.load("xlsx", &knowing, vec![], None),
            &bytes,
            &LIMITS,
        )
        .unwrap_err();
    assert!(rejected.problems[0].message.contains("merged range A2:A3"));

    // Rows that hold nothing after the data are not rows; one inside is a problem.
    let trailing = format!("{good}<row r=\"4\"/><row r=\"9\"><c r=\"A9\" s=\"1\"/></row>");
    let bytes = Book::new(&stock_sheet(&trailing)).bytes();
    assert_eq!(sources.read(&def, &bytes, &LIMITS).unwrap().rows.len(), 2);
    let gap = format!(
        "{}{}",
        stock_row(2, "EX-1001", "12", "46343"),
        stock_row(4, "EX-1002", "7", "46344")
    );
    let bytes = Book::new(&stock_sheet(&gap)).bytes();
    let rejected = sources.read(&def, &bytes, &LIMITS).unwrap_err();
    assert_eq!(rejected.problems.len(), 1);
    assert_eq!(rejected.problems[0].at, Some(CellRef::new(0, 3, 1)));
}

/// A crafted or damaged workbook stops; it is never read by a default.
#[test]
fn a_workbook_whose_attributes_break_the_format_is_refused_not_read_by_default() {
    let sources = Sources::builtin();
    let mut target = Target::new();
    let fields = stock_fields(&mut target);
    let good = Book::new(&stock_sheet(&stock_row(2, "EX-1", "5", "45000")));
    let def = {
        let bytes = good.bytes();
        let spec = spec("xlsx", &input(&bytes), Locate::Sheet("Stock".to_owned()), fields);
        let def = target.load("xlsx", &spec, vec![], None);
        assert_eq!(sources.read(&def, &bytes, &LIMITS).unwrap().rows.len(), 1);
        def
    };
    let row = stock_row(2, "EX-1", "5", "45000");
    let broken = [
        // A hidden row whose flag is not quoted: once read as a shown row.
        row.replacen("<row r=\"2\">", "<row r=\"2\" hidden=1>", 1),
        // A flag that is neither true nor false.
        row.replacen("<row r=\"2\">", "<row r=\"2\" hidden=\"yes\">", 1),
        // A style the workbook lacks: once read as a plain number, not a date.
        row.replacen("s=\"1\"", "s=\"7\"", 1),
        row.replacen("s=\"1\"", "s=\"one\"", 1),
        // A cell closed by another element's tag.
        row.replacen("</v></c></row>", "</v></row></c>", 1),
    ];
    for rows in broken {
        assert_ne!(rows, row);
        let bytes = Book::new(&stock_sheet(&rows)).bytes();
        let failure = sources.read(&def, &bytes, &LIMITS).unwrap_err().failure;
        assert_eq!(failure.code(), &SOURCE_MALFORMED, "{rows}");
    }
}

#[test]
fn deflated_parts_shared_strings_and_spans_read_the_same() {
    let rows = "<row r=\"2\"><c r=\"A2\" t=\"s\"><v>1</v></c><c r=\"B2\"><v>1.50</v></c><c r=\"C2\" t=\"b\"><v>1</v></c></row>\
                <row r=\"3\"><c r=\"A3\" t=\"s\"><v>0</v></c><c r=\"B3\" t=\"str\"><f>\"a\"&amp;\"b\"</f><v>a &amp; b</v></c><c r=\"C3\" t=\"e\"><v>#DIV/0!</v></c></row>";
    let shared: &[&str] = &[
        "<si><t>plain</t></si>",
        "<si><r><rPr><b/></rPr><t>rich </t></r><r><t xml:space=\"preserve\">text_x000D_</t></r><rPh><t>guide</t></rPh></si>",
    ];
    let stored = Book {
        shared,
        ..Book::new(&stock_sheet(rows))
    };
    let deflated = Book {
        packed: Packed::Deflated,
        shared,
        ..Book::new(&stock_sheet(rows))
    };
    let sources = Sources::builtin();
    let parsed = sources.parse("xlsx", &input(&stored.bytes())).unwrap();
    assert_eq!(
        parsed,
        sources.parse("xlsx", &input(&deflated.bytes())).unwrap()
    );
    assert!(deflated.bytes().len() < stored.bytes().len());

    let part = &parsed.parts[0];
    assert_eq!(part.path, "xl/worksheets/sheet1.xml");
    let raw = |row, column| part.cell(row, column).unwrap().cell.raw.clone();
    assert_eq!(raw(2, 1), RawCell::Text("rich text\r".to_owned()));
    assert_eq!(part.cell(2, 1).unwrap().shared, Some(1));
    assert_eq!(raw(2, 2), RawCell::Number("1.50".to_owned()));
    assert_eq!(raw(2, 3), RawCell::Bool(true));
    assert_eq!(raw(3, 1), RawCell::Text("plain".to_owned()));
    assert_eq!(
        raw(3, 2),
        RawCell::Formula {
            cached: Some(Box::new(RawCell::Text("a & b".to_owned())))
        }
    );
    assert_eq!(raw(3, 3), RawCell::Error("#DIV/0!".to_owned()));

    // Every span covers exactly the literal of its value, in the part as
    // unpacked; writing each span's own bytes back changes nothing.
    let bytes = part.bytes(&[]);
    let literal = |row, column| {
        let span = part.cell(row, column).unwrap().value_span.unwrap();
        std::str::from_utf8(span.of(bytes).unwrap()).unwrap()
    };
    assert_eq!(literal(1, 1), "code");
    assert_eq!(literal(2, 1), "1");
    assert_eq!(literal(2, 2), "1.50");
    assert_eq!(literal(2, 3), "1");
    assert_eq!(literal(3, 2), "a &amp; b");
    assert_eq!(literal(3, 3), "#DIV/0!");
    let mut rebuilt = bytes.to_vec();
    let mut last_end = 0;
    for placed in &part.cells {
        let span = placed.value_span.unwrap();
        assert!(span.start >= last_end);
        last_end = span.end;
        let own = span.of(bytes).unwrap().to_vec();
        rebuilt.splice(span.start as usize..span.end as usize, own);
    }
    assert_eq!(rebuilt, bytes);
    // The names the spreadsheet program keeps for itself are not layout,
    // and a link to another file is never followed.
    assert_eq!(parsed.structure.names.len(), 1);
}

// ------------------------------------------------------------ fingerprints

#[test]
fn a_changed_header_changes_the_fingerprint_and_stops_with_the_differences() {
    let sources = Sources::builtin();
    let mut target = Target::new();
    let text = target.text();
    let integer = target.integer();

    // Delimited text: the stand's stock file, and the same with one header renamed.
    let bytes = seed_file(seed::ARCHIVED_STOCK_CSV);
    let fields = vec![
        column("item_code", "item_code", text, true),
        column("on_hand", "on_hand", integer, true),
    ];
    let spec_csv = spec("csv", &input(&bytes), Locate::Document, fields);
    let def = target.load("csv", &spec_csv, vec![], None);
    assert_eq!(sources.read(&def, &bytes, &LIMITS).unwrap().rows.len(), 3);
    let renamed = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("on_hand", "quantity_on_hand");
    let before = sources.describe("csv", &input(&bytes)).unwrap().fingerprint;
    let after = sources
        .describe("csv", &input(renamed.as_bytes()))
        .unwrap()
        .fingerprint;
    assert_ne!(before.digest, after.digest);
    let rejected = sources.read(&def, renamed.as_bytes(), &LIMITS).unwrap_err();
    assert_eq!(rejected.failure.code(), &SOURCE_TEMPLATE_UNKNOWN);
    assert_eq!(rejected.failure.class(), a2m_core::FailureClass::Input);
    assert_eq!(rejected.problems.len(), 1);
    assert!(rejected.problems[0].message.contains("`on_hand`"));
    // The input's own text is not quoted: only the template's.
    assert!(!rejected.problems[0].message.contains("quantity_on_hand"));
    // A column added, a column taken away: each is listed.
    let added = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("on_hand\n", "on_hand,note\n");
    let rejected = sources.read(&def, added.as_bytes(), &LIMITS).unwrap_err();
    assert_eq!(rejected.failure.code(), &SOURCE_TEMPLATE_UNKNOWN);
    assert_eq!(rejected.problems.len(), 1);
    // Values do not enter the fingerprint.
    let other_values = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("15", "16");
    assert_eq!(
        before.digest,
        sources
            .describe("csv", &input(other_values.as_bytes()))
            .unwrap()
            .fingerprint
            .digest
    );

    // A workbook: the stand's other layout renames a sheet and a header.
    let book = seed_file(seed::PRICE_LIST_WORKBOOK);
    let amount = target.decimal(2);
    let fields = ["Item code", "Length", "Unit price", "On hand", "Line value"]
        .into_iter()
        .zip([text, text, amount, amount, amount])
        .map(|(name, field_type)| column(name, name, field_type, false))
        .collect();
    let spec_book = spec(
        "xlsx",
        &input(&book),
        Locate::Named("price_table".to_owned()),
        fields,
    );
    let def = target.load("xlsx", &spec_book, vec![], None);
    let other = seed::workbook(&seed::Workbook {
        other_layout: true,
        ..seed::Workbook::default()
    });
    let rejected = sources.read(&def, &other, &LIMITS).unwrap_err();
    assert_eq!(rejected.failure.code(), &SOURCE_TEMPLATE_UNKNOWN);
    let messages: Vec<&str> = rejected
        .problems
        .iter()
        .map(|p| p.message.as_str())
        .collect();
    // Each difference names the template's own text and the place; what the
    // input holds there is not quoted.
    assert!(
        messages
            .iter()
            .any(|m| m.contains("`Prices|shown`") && m.contains("sheet 1")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("`Unit price`") && m.contains("column 3")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("Price list") || m.contains("`Price`")),
        "{messages:?}"
    );
    // A value changed and nothing else: the same template.
    let other_price = seed::workbook(&seed::Workbook {
        unit_price_ex_1002: "11",
        ..seed::Workbook::default()
    });
    assert!(sources.read(&def, &other_price, &LIMITS).is_ok());

    // A second known version is matched by its own fingerprint.
    let both = SourceSpec {
        versions: vec![
            spec_csv.versions[0].clone(),
            TemplateVersion {
                version: "2".to_owned(),
                fingerprint: after,
            },
        ],
        columns: vec!["item_code".to_owned(), "quantity_on_hand".to_owned()],
        fields: vec![
            column("item_code", "item_code", text, true),
            column("on_hand", "quantity_on_hand", integer, true),
        ],
        ..spec_csv
    };
    let def = target.load("csv", &both, vec![], None);
    let records = sources.read(&def, renamed.as_bytes(), &LIMITS).unwrap();
    assert_eq!(records.version.as_deref(), Some("2"));
}

#[test]
fn the_same_bytes_give_the_same_result_and_a_fixed_fingerprint() {
    let sources = Sources::builtin();
    let book = seed_file(seed::PRICE_LIST_WORKBOOK);
    let first = sources.parse("xlsx", &input(&book)).unwrap();
    let second = sources.parse("xlsx", &input(&book)).unwrap();
    assert_eq!(format!("{first:?}"), format!("{second:?}"));
    // These digests are the same on every operating system: nothing but the
    // bytes enters them. A change to what a fingerprint covers changes them.
    assert_eq!(
        first.structure.fingerprint().digest.to_string(),
        "a512db41749f769b890583cbae13ba5e8bb5196cf5e060917fd4d6aa4a757f8f"
    );
    let csv = seed_file(seed::STOCK_CSV);
    assert_eq!(
        sources
            .describe("csv", &input(&csv))
            .unwrap()
            .fingerprint
            .digest
            .to_string(),
        "e3ffafe814693c41ba98dd9ca62e0a0cc1adb14c3dc969a4a4cda43a70092267"
    );

    let mut target = Target::new();
    let text = target.text();
    let fields = ["item_code", "description", "on_hand", "note"]
        .into_iter()
        .map(|name| column(name, name, text, false))
        .collect();
    let def = target.load(
        "csv",
        &spec("csv", &input(&csv), Locate::Document, fields),
        vec![],
        None,
    );
    let once = sources.read(&def, &csv, &LIMITS).unwrap();
    let twice = sources.read(&def, &csv, &LIMITS).unwrap();
    assert_eq!(once, twice);
    assert_eq!(
        format!("{once:?}").into_bytes(),
        format!("{twice:?}").into_bytes()
    );
}

// ---------------------------------------------------------------- problems

#[test]
fn every_problem_is_reported_in_one_pass_with_its_position_and_reason() {
    let template = b"code,qty,day\nEX-1,1,2026-11-17\n";
    let mut target = Target::new();
    let mut spec = spec(
        "csv",
        &input(template),
        Locate::Document,
        stock_fields(&mut target),
    );
    spec.key = vec!["code".to_owned()];
    let def = target.load("csv", &spec, vec![], None);
    let bytes = b"code,qty,day\n\
                  EX-1,12,2026-11-17\n\
                  EX-2,twelve,2026-11-18\n\
                  ,7,not a day\n\
                  EX-1,3,\n\
                  EX-4,1.5\n\
                  EX-5,99999999999999999999999999999999999,\n";
    let rejected = Sources::builtin().read(&def, bytes, &LIMITS).unwrap_err();
    let found: Vec<(u32, u32, Option<&str>, &str)> = rejected
        .problems
        .iter()
        .map(|problem| {
            let at = problem.at.unwrap();
            (
                at.row,
                at.column,
                problem.field.as_deref(),
                problem.code.as_str(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            // The width of a row is checked where the data is located, so it
            // comes first and alone: no row is typed from a file whose rows
            // do not line up.
            (6, 1, None, "schema_invalid"),
        ]
    );

    // With that row made whole, every other problem comes back at once.
    let bytes = String::from_utf8(bytes.to_vec())
        .unwrap()
        .replace("EX-4,1.5\n", "EX-4,1.5,\n");
    let rejected = Sources::builtin()
        .read(&def, bytes.as_bytes(), &LIMITS)
        .unwrap_err();
    let found: Vec<(u32, u32, Option<&str>, &str)> = rejected
        .problems
        .iter()
        .map(|problem| {
            let at = problem.at.unwrap();
            (
                at.row,
                at.column,
                problem.field.as_deref(),
                problem.code.as_str(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (3, 2, Some("qty"), "value_unparsable"),
            (4, 1, Some("code"), "schema_invalid"),
            (4, 3, Some("day"), "value_unparsable"),
            (5, 1, None, "key_not_unique"),
            (7, 2, Some("qty"), "value_out_of_range"),
        ]
    );
    assert_eq!(rejected.failure.class(), a2m_core::FailureClass::Input);
    // No message holds a value from the input.
    let shown = rejected.to_string();
    for value in ["twelve", "not a day", "EX-2", "9999999"] {
        assert!(!shown.contains(value), "{shown}");
    }
    assert!(
        rejected
            .problems
            .iter()
            .all(|problem| !problem.message.is_empty())
    );
}

#[test]
fn headers_are_found_by_text_and_never_guessed() {
    let sources = Sources::builtin();
    let mut target = Target::new();
    let template = b"code,qty,day\n";
    let mut loose = spec(
        "csv",
        &input(template),
        Locate::Document,
        stock_fields(&mut target),
    );
    let problems = |bytes: &[u8], spec: &mut SourceSpec, target: &Target| {
        // The definition is made to know the layout, so that what is tested
        // is the locating and not the fingerprint.
        spec.versions[0].fingerprint = Sources::builtin()
            .describe("csv", &input(bytes))
            .unwrap()
            .fingerprint;
        let def = target.load("csv", spec, vec![], None);
        sources
            .read(&def, bytes, &LIMITS)
            .unwrap_err()
            .problems
            .into_iter()
            .map(|problem| (problem.field.unwrap_or_default(), problem.message))
            .collect::<Vec<_>>()
    };
    let twice = problems(b"code,qty,qty,day\n", &mut loose, &target);
    assert!(
        twice
            .iter()
            .any(|(field, message)| field == "qty" && message.contains("twice"))
    );
    let missing = problems(b"code,day\n", &mut loose, &target);
    assert!(
        missing
            .iter()
            .any(|(field, message)| field == "qty" && message.contains("not in the data"))
    );
    let unknown = problems(b"code,qty,day,extra\n", &mut loose, &target);
    assert!(
        unknown
            .iter()
            .any(|(field, message)| field == "extra" && message.contains("does not name"))
    );
    let unnamed = problems(b"code,qty,day,\n", &mut loose, &target);
    assert!(
        unnamed
            .iter()
            .any(|(_, message)| message.contains("no header"))
    );
    let empty = problems(b"", &mut loose, &target);
    assert!(
        empty
            .iter()
            .any(|(_, message)| message.contains("no header row"))
    );
}

#[test]
fn text_such_as_na_is_text_unless_the_definition_lists_it_as_missing() {
    let sources = Sources::builtin();
    let mut target = Target::new();
    let text = target.text();
    let fields = vec![
        column("code", "code", text, true),
        column("note", "note", text, false),
    ];
    let csv = b"code,note\nEX-1,NA\nEX-2,null\nEX-3,n/a\n".to_vec();
    let json = br#"[{"code": "EX-1", "note": "NA"}, {"code": "EX-2", "note": "null"}, {"code": "EX-3", "note": "n/a"}]"#.to_vec();
    let rows = "<row r=\"2\"><c r=\"A2\" t=\"inlineStr\"><is><t>EX-1</t></is></c><c r=\"B2\" t=\"inlineStr\"><is><t>NA</t></is></c></row>\
                <row r=\"3\"><c r=\"A3\" t=\"inlineStr\"><is><t>EX-2</t></is></c><c r=\"B3\" t=\"inlineStr\"><is><t>null</t></is></c></row>\
                <row r=\"4\"><c r=\"A4\" t=\"inlineStr\"><is><t>EX-3</t></is></c><c r=\"B4\" t=\"inlineStr\"><is><t>n/a</t></is></c></row>";
    let sheet = format!(
        "<sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>code</t></is></c><c r=\"B1\" t=\"inlineStr\"><is><t>note</t></is></c></row>{rows}</sheetData>"
    );
    let book = Book::new(&sheet).bytes();
    for (reader, bytes, locate) in [
        ("csv", &csv, Locate::Document),
        ("json", &json, Locate::Document),
        ("xlsx", &book, Locate::Sheet("Stock".to_owned())),
    ] {
        let plain = spec(reader, &input(bytes), locate, fields.clone());
        let records = sources
            .read(&target.load(reader, &plain, vec![], None), bytes, &LIMITS)
            .unwrap();
        let notes: Vec<&Value> = (0..3).map(|row| field(&records, row, "note")).collect();
        assert_eq!(
            notes,
            [
                &Value::from("NA"),
                &Value::from("null"),
                &Value::from("n/a")
            ],
            "{reader}"
        );

        let listing = SourceSpec {
            missing: vec!["NA".to_owned()],
            ..plain
        };
        let records = sources
            .read(&target.load(reader, &listing, vec![], None), bytes, &LIMITS)
            .unwrap();
        let notes: Vec<&Value> = (0..3).map(|row| field(&records, row, "note")).collect();
        assert_eq!(
            notes,
            [&Value::Null, &Value::from("null"), &Value::from("n/a")],
            "{reader}"
        );
    }
}

#[test]
fn a_file_of_another_format_is_refused_whatever_reader_it_is_given_to() {
    let sources = Sources::builtin();
    let csv = seed_file(seed::STOCK_CSV);
    let json = br#"[{"code": "EX-1"}]"#.to_vec();
    let book = seed_file(seed::PRICE_LIST_WORKBOOK);
    for (reader, bytes) in [
        ("json", &csv),
        ("xlsx", &csv),
        ("csv", &json),
        ("xlsx", &json),
        ("csv", &book),
        ("json", &book),
    ] {
        let failure = sources.parse(reader, &input(bytes)).unwrap_err();
        assert_eq!(failure.code(), &FILE_TYPE_MISMATCH, "{reader}");
    }
    // A package that is not a workbook.
    let other = package(&[("word/document.xml", b"<w/>".as_slice(), Packed::Stored)]);
    assert_eq!(
        sources.parse("xlsx", &input(&other)).unwrap_err().code(),
        &FILE_TYPE_MISMATCH
    );
}

#[test]
fn limits_stop_an_input_that_is_too_large() {
    let sources = Sources::builtin();
    let large = seed_file(seed::LARGE_CSV);
    assert_eq!(
        sources.parse("csv", &input(&large)).unwrap().parts[0].rows as usize,
        seed::LARGE_ROWS + 1
    );
    let code = |reader: &str, bytes: &[u8], limits: Limits| {
        sources
            .parse(
                reader,
                &SourceInput {
                    bytes,
                    limits,
                    options: ReaderOptions::default(),
                },
            )
            .unwrap_err()
            .code()
            .clone()
    };
    assert_eq!(
        code(
            "csv",
            &large,
            Limits {
                bytes: 1024,
                ..LIMITS
            }
        ),
        SOURCE_TOO_LARGE
    );
    assert_eq!(
        code(
            "csv",
            &large,
            Limits {
                rows: 1000,
                ..LIMITS
            }
        ),
        SOURCE_TOO_LARGE
    );
    assert_eq!(
        code(
            "csv",
            &large,
            Limits {
                cells: 1000,
                ..LIMITS
            }
        ),
        SOURCE_TOO_LARGE
    );
    let book = seed_file(seed::PRICE_LIST_WORKBOOK);
    assert_eq!(
        code(
            "xlsx",
            &book,
            Limits {
                unpacked_bytes: 512,
                ..LIMITS
            }
        ),
        SOURCE_TOO_LARGE
    );
    assert_eq!(
        code("xlsx", &book, Limits { rows: 3, ..LIMITS }),
        SOURCE_TOO_LARGE
    );
    assert_eq!(
        code(
            "xlsx",
            &book,
            Limits {
                cells: 10,
                ..LIMITS
            }
        ),
        SOURCE_TOO_LARGE
    );
    assert_eq!(
        code(
            "xlsx",
            &book,
            Limits {
                cell_bytes: 4,
                ..LIMITS
            }
        ),
        SOURCE_TOO_LARGE
    );
    let json = seed_file(seed::UNITS_JSON);
    assert_eq!(
        code("json", &json, Limits { cells: 2, ..LIMITS }),
        SOURCE_TOO_LARGE
    );
}

// ---------------------------------------------- json, rows handed in, typed

#[test]
fn a_json_list_under_a_key_is_read_like_any_other_source() {
    let bytes = br#"{"exported": "2026-11-17", "lines": [
        {"code": "EX-1", "qty": 2.50, "day": "2026-11-17"},
        {"qty": 1.9e1, "code": "EX-2", "day": null}
    ]}"#;
    let mut target = Target::new();
    let spec = spec(
        "json",
        &input(bytes),
        Locate::Named("lines".to_owned()),
        stock_fields(&mut target),
    );
    let def = target.load("json", &spec, vec![], None);
    let records = Sources::builtin().read(&def, bytes, &LIMITS).unwrap();
    assert_eq!(field(&records, 0, "qty"), &decimal("2.5"));
    assert_eq!(field(&records, 1, "qty"), &decimal("19"));
    assert_eq!(field(&records, 1, "code"), &Value::from("EX-2"));
    assert_eq!(
        field(&records, 0, "day"),
        &Value::Date(Date::new(2026, 11, 17).unwrap())
    );
    assert_eq!(field(&records, 1, "day"), &Value::Null);

    // A number where a date is expected has no date system to be read by,
    // and a list where one value is expected is never typed.
    let wrong = br#"{"lines": [{"code": "EX-1", "qty": 1, "day": 46343}, {"code": ["EX-2"], "qty": 1, "day": null}]}"#;
    let rejected = Sources::builtin().read(&def, wrong, &LIMITS).unwrap_err();
    let codes: Vec<&str> = rejected.problems.iter().map(|p| p.code.as_str()).collect();
    assert_eq!(codes, ["schema_invalid", "schema_invalid"]);
    assert!(rejected.problems[0].message.contains("no date system"));
    assert!(rejected.problems[1].message.contains("a list or a record"));
}

#[test]
fn rows_handed_in_by_a_caller_go_through_the_same_validation() {
    let mut target = Target::new();
    let fields = stock_fields(&mut target);
    let spec = SourceSpec {
        versions: vec![],
        locate: Locate::Document,
        columns: vec!["code".to_owned(), "qty".to_owned(), "day".to_owned()],
        fields,
        key: vec!["code".to_owned()],
        missing: vec![],
        hidden: HiddenRule::Reject,
        options: ReaderOptions::default(),
    };
    let def = target.load(OPERATION_READER, &spec, vec![], None);
    let row = |code: &str, qty: Value| {
        Values::new()
            .with("code", code)
            .with("qty", qty)
            .with("day", "2026-11-17")
    };
    let sources = Sources::builtin();
    let records = sources
        .from_rows(
            &def,
            vec![
                row("EX-1", Value::from("1,234.5")),
                row("EX-2", decimal("7.25")),
            ],
        )
        .unwrap();
    assert_eq!(field(&records, 0, "qty"), &decimal("1234.5"));
    assert_eq!(field(&records, 1, "qty"), &decimal("7.25"));
    assert_eq!(records.version, None);
    assert_eq!(records.fingerprint, None);

    let rejected = sources
        .from_rows(
            &def,
            vec![
                row("EX-1", Value::from("1")),
                row("EX-1", Value::from("2")),
                row("EX-3", Value::from("x")).with("extra", "?"),
                Values::new().with("qty", "1"),
            ],
        )
        .unwrap_err();
    let found: Vec<(u32, Option<&str>, &str)> = rejected
        .problems
        .iter()
        .map(|p| (p.at.unwrap().row, p.field.as_deref(), p.code.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (2, None, "key_not_unique"),
            (3, Some("extra"), "schema_invalid"),
            (3, Some("qty"), "value_unparsable"),
            (4, Some("code"), "schema_invalid"),
        ]
    );
    // A file reader needs a template version; rows handed in have none.
    assert_eq!(
        target
            .try_load("csv", &spec, vec![], None)
            .unwrap_err()
            .code(),
        &SOURCE_DEFINITION_INVALID
    );
}

#[test]
fn typed_is_the_same_code_read_uses_and_refuses_what_it_cannot_take() {
    let mut target = Target::new();
    let text = FieldTyper::load(&target.plan.clone(), target.text()).unwrap_err();
    assert_eq!(text.code(), &SOURCE_DEFINITION_INVALID);
    let text_index = target.text();
    let integer_index = target.integer();
    let date_index = target.date();
    let text = FieldTyper::load(&target.plan, text_index).unwrap();
    let integer = FieldTyper::load(&target.plan, integer_index).unwrap();
    let date = FieldTyper::load(&target.plan, date_index).unwrap();
    let cell = |raw: RawCell| Cell {
        raw,
        at: CellRef::new(0, 2, 3),
        format: None,
        hidden: false,
        merged: None,
    };
    let workbook = CellContext {
        date_system: Some(a2m_expr::DateSystem::From1900),
        missing: &[],
    };
    let plain = CellContext::default();
    let number = |text: &str| cell(RawCell::Number(text.to_owned()));

    assert_eq!(typed(&number("40"), &integer, &plain), Ok(Value::Int(40)));
    assert_eq!(
        typed(&number("4.0E1"), &integer, &plain),
        Ok(Value::Int(40))
    );
    assert_eq!(
        typed(&number("1002"), &text, &plain),
        Ok(Value::from("1002"))
    );
    assert_eq!(
        typed(&cell(RawCell::Empty), &integer, &plain),
        Ok(Value::Null)
    );
    assert_eq!(
        typed(&number("46343"), &date, &workbook),
        Ok(Value::Date(Date::new(2026, 11, 17).unwrap()))
    );
    assert_eq!(
        typed(
            &cell(RawCell::Formula {
                cached: Some(Box::new(RawCell::Number("7".to_owned())))
            }),
            &integer,
            &plain
        ),
        Ok(Value::Int(7))
    );
    let refused = |cell: &Cell, typer: &FieldTyper, context: &CellContext<'_>| {
        let problem = typed(cell, typer, context).unwrap_err();
        assert_eq!(problem.at, Some(CellRef::new(0, 2, 3)));
        problem.code
    };
    // A number with a fraction has more than one spelling as text.
    assert_eq!(refused(&number("1.50"), &text, &plain), SCHEMA_INVALID);
    assert_eq!(
        refused(&number("1.5"), &integer, &plain),
        a2m_expr::VALUE_UNPARSABLE
    );
    assert_eq!(refused(&number("46343"), &date, &plain), SCHEMA_INVALID);
    // Serial 60 is a day that never was.
    assert_eq!(refused(&number("60"), &date, &workbook), VALUE_OUT_OF_RANGE);
    assert_eq!(
        refused(
            &cell(RawCell::DateSerial("46343".to_owned())),
            &integer,
            &workbook
        ),
        SCHEMA_INVALID
    );
    assert_eq!(
        refused(&cell(RawCell::Bool(true)), &integer, &plain),
        SCHEMA_INVALID
    );
    assert_eq!(
        refused(&cell(RawCell::Error("#N/A".to_owned())), &text, &plain),
        SCHEMA_INVALID
    );
    assert_eq!(
        refused(&cell(RawCell::Formula { cached: None }), &text, &plain),
        FORMULA_WITHOUT_VALUE
    );
}

#[test]
fn a_definition_that_does_not_hold_together_does_not_load() {
    let mut target = Target::new();
    let template = b"code,qty,day\n";
    let good = spec(
        "csv",
        &input(template),
        Locate::Document,
        stock_fields(&mut target),
    );
    assert!(target.try_load("csv", &good, vec![], None).is_ok());
    let refused = |target: &Target, spec: &SourceSpec, mapping: Vec<StepArg>| {
        target
            .try_load("csv", spec, mapping, None)
            .unwrap_err()
            .code()
            .clone()
    };
    let mut twice = good.clone();
    twice.fields.push(twice.fields[0].clone());
    assert_eq!(refused(&target, &twice, vec![]), SOURCE_DEFINITION_INVALID);
    let mut unlisted = good.clone();
    unlisted.columns.pop();
    assert_eq!(
        refused(&target, &unlisted, vec![]),
        SOURCE_DEFINITION_INVALID
    );
    let mut wrong_key = good.clone();
    wrong_key.key = vec!["nothing".to_owned()];
    assert_eq!(
        refused(&target, &wrong_key, vec![]),
        SOURCE_DEFINITION_INVALID
    );
    let mut dangling = good.clone();
    dangling.fields[0].field_type = FieldTypeIndex::new(99);
    assert_eq!(
        refused(&target, &dangling, vec![]),
        SOURCE_DEFINITION_INVALID
    );
    let mut unsound = good.clone();
    unsound.versions[0].fingerprint.parts.pop();
    assert_eq!(
        refused(&target, &unsound, vec![]),
        SOURCE_DEFINITION_INVALID
    );
    let stray = vec![StepArg {
        name: "nothing".to_owned(),
        value: ExprIndex::new(0),
    }];
    assert_eq!(refused(&target, &good, stray), SOURCE_DEFINITION_INVALID);

    // A registry refuses two readers with one name.
    let readers: Vec<Arc<dyn SourceReader>> =
        vec![Arc::new(CsvReader::new()), Arc::new(CsvReader::new())];
    assert_eq!(
        Sources::new(readers).unwrap_err().code(),
        &SOURCE_DEFINITION_INVALID
    );
    let names: Vec<String> = Sources::builtin()
        .readers()
        .map(ToString::to_string)
        .collect();
    assert_eq!(names, ["csv", "json", "xlsx"]);
}
