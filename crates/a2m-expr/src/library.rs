//! The host-function library: a closed table, versioned with the engine.
//!
//! Each function has a name, a signature, and a sentence. The type checker
//! and `docs.get` both read this table; nothing outside this crate can add
//! to it.

use std::fmt;

/// The version of the host-function library an expression was checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LibraryVersion(u32);

impl LibraryVersion {
    /// The library this engine carries.
    pub const CURRENT: Self = Self(1);

    /// The library with that number, when this engine carries it.
    #[must_use]
    pub const fn new(number: u32) -> Option<Self> {
        if number == Self::CURRENT.0 {
            Some(Self(number))
        } else {
            None
        }
    }

    /// The number a plan stores.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for LibraryVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The name of a host function. Only the library makes one, so a name that
/// exists is a function that exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostFnName(&'static str);

impl HostFnName {
    /// The name as an expression writes it.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for HostFnName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Which function a table row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FnId {
    Decimal,
    ParseDecimal,
    ParseInt,
    ParseMoney,
    ParseBool,
    RoundTo,
    Div,
    ToInches,
    ToFeetInches,
    ToUnit,
    SnapToStep,
    ParseDate,
    FormatDate,
    ParseDatetime,
    FormatDatetime,
    ParseTime,
    ExcelSerialToDate,
    Trim,
    Lower,
    Upper,
    Nfc,
    CollapseSpace,
    CodeFor,
    LabelFor,
    Coalesce,
    Size,
    AsText,
    AsInteger,
    AsDecimal,
    AsMoney,
    AsMeasurement,
    AsDate,
    AsDatetime,
    AsTime,
    AsBoolean,
    AsChoice,
}

/// What an argument must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Param {
    /// Any value; the function says at run time what it can read.
    Any,
    Text,
    Decimal,
    /// A whole number or a decimal.
    Number,
    Date,
    Instant,
    // The rest are rules, written as literals so that they are checked once,
    // when the expression is compiled.
    /// A whole number from 0 to 28.
    Places,
    /// A rounding mode's name.
    Rounding,
    /// `"."` or `","`.
    Mark,
    /// A unit's name.
    Unit,
    /// A unit's name, or `"ft_in"`.
    DisplayUnit,
    DateFormat,
    TimeFormat,
    DatetimeFormat,
    Zone,
    /// `"1900"` or `"1904"`.
    DateSystem,
    /// `"sensitive"` or `"insensitive"`.
    Case,
    /// `"collapse"`, `"trim"`, or `"exact"`.
    Whitespace,
    /// A record literal of stored code to label.
    Codes,
    /// A three-letter currency code.
    Currency,
    /// Literal text.
    Label,
}

impl Param {
    /// Whether the argument is a rule that must be written as a literal.
    pub(crate) const fn is_rule(self) -> bool {
        !matches!(
            self,
            Self::Any | Self::Text | Self::Decimal | Self::Number | Self::Date | Self::Instant
        )
    }
}

/// What a function gives back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ret {
    Bool,
    Int,
    Text,
    Decimal,
    Date,
    Instant,
    Duration,
    Choice,
    /// The common type of the arguments.
    Same,
}

impl Ret {
    const fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Text => "text",
            Self::Decimal => "decimal",
            Self::Date => "date",
            Self::Instant => "instant",
            Self::Duration => "duration",
            Self::Choice => "choice",
            Self::Same => "the arguments' type",
        }
    }
}

/// One host function.
#[derive(Debug)]
pub struct HostFn {
    pub(crate) id: FnId,
    name: &'static str,
    group: &'static str,
    pub(crate) params: &'static [(&'static str, Param)],
    /// How many of the arguments must be given; the rest have defaults.
    pub(crate) required: usize,
    pub(crate) ret: Ret,
    summary: &'static str,
}

impl HostFn {
    /// The function's name.
    #[must_use]
    pub const fn name(&self) -> HostFnName {
        HostFnName(self.name)
    }

    /// The group it is documented under.
    #[must_use]
    pub const fn group(&self) -> &'static str {
        self.group
    }

    /// What it does, in a sentence or two.
    #[must_use]
    pub const fn summary(&self) -> &'static str {
        self.summary
    }

    /// How it is called, such as `div(a, b, places, rounding) -> decimal`.
    #[must_use]
    pub fn signature(&self) -> String {
        let mut out = format!("{}(", self.name);
        for (index, (name, _)) in self.params.iter().enumerate() {
            if index == self.required && index > 0 {
                out.push('[');
            }
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(name);
        }
        if self.id == FnId::Coalesce {
            out.push_str(", ...");
        }
        if self.required < self.params.len() {
            out.push(']');
        }
        out.push_str(") -> ");
        out.push_str(self.ret.name());
        out
    }
}

/// The host-function library.
#[derive(Debug)]
pub struct Library {
    version: LibraryVersion,
    cost_limit: u64,
    functions: &'static [HostFn],
}

impl Library {
    /// The library's version.
    #[must_use]
    pub const fn version(&self) -> LibraryVersion {
        self.version
    }

    /// The most work one evaluation may do, in cost units. One unit is one
    /// node of the expression visited, one item passed over, or 64 bytes of
    /// text handled by a function.
    #[must_use]
    pub const fn cost_limit(&self) -> u64 {
        self.cost_limit
    }

    /// Every function, in the order the documents list them.
    #[must_use]
    pub const fn functions(&self) -> &'static [HostFn] {
        self.functions
    }

    /// The function with that name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&'static HostFn> {
        self.functions.iter().find(|function| function.name == name)
    }
}

/// The most work one evaluation may do. Not yet measured on a pipeline; see
/// the crate design's build order, step 6.
pub const COST_LIMIT: u64 = 1_000_000;

const fn function(
    id: FnId,
    name: &'static str,
    group: &'static str,
    params: &'static [(&'static str, Param)],
    required: usize,
    ret: Ret,
    summary: &'static str,
) -> HostFn {
    HostFn {
        id,
        name,
        group,
        params,
        required,
        ret,
        summary,
    }
}

use Param as P;

static FUNCTIONS: [HostFn; 36] = [
    function(FnId::ToInches, "to_inches", "measurement", &[("text", P::Text)], 1, Ret::Decimal,
        "Reads a length written in feet and inches, such as 7' 6 1/2\", as inches. The marks are required."),
    function(FnId::ToFeetInches, "to_feet_inches", "measurement", &[("inches", P::Decimal), ("step", P::Decimal)], 2, Ret::Text,
        "Writes inches as feet and inches to the nearest step, such as 7' 6 1/2\"."),
    function(FnId::ToUnit, "to_unit", "measurement",
        &[("value", P::Decimal), ("from", P::Unit), ("to", P::Unit), ("places", P::Places), ("rounding", P::Rounding)], 5, Ret::Decimal,
        "Converts a measurement between two units of one kind, rounding once to the places given."),
    function(FnId::SnapToStep, "snap_to_step", "measurement",
        &[("value", P::Decimal), ("step", P::Decimal), ("rounding", P::Rounding)], 3, Ret::Decimal,
        "The multiple of the step nearest to the value, such as the nearest 1/16."),
    function(FnId::Decimal, "decimal", "numbers", &[("value", P::Any)], 1, Ret::Decimal,
        "A whole number as a decimal, or a decimal written as literal text: decimal(\"1.5\"). Text that is not a literal goes through parse_decimal."),
    function(FnId::ParseDecimal, "parse_decimal", "numbers",
        &[("text", P::Text), ("places", P::Places), ("rounding", P::Rounding), ("mark", P::Mark)], 3, Ret::Decimal,
        "Reads a displayed number, strips well-formed grouping, and rounds to the places given. The mark is the decimal mark, \".\" unless given."),
    function(FnId::ParseInt, "parse_int", "numbers", &[("text", P::Text), ("mark", P::Mark)], 1, Ret::Int,
        "Reads a displayed whole number, stripping well-formed grouping."),
    function(FnId::ParseMoney, "parse_money", "numbers",
        &[("text", P::Text), ("currency", P::Currency), ("places", P::Places), ("rounding", P::Rounding), ("mark", P::Mark)], 4, Ret::Decimal,
        "Reads displayed money in the field's currency. Its code or one currency symbol may stand at either end; another currency stops."),
    function(FnId::ParseBool, "parse_bool", "numbers",
        &[("text", P::Text), ("true_text", P::Label), ("false_text", P::Label)], 3, Ret::Bool,
        "Reads displayed text as true or false by the two labels given, ignoring case. Any other text stops."),
    function(FnId::RoundTo, "round_to", "numbers",
        &[("value", P::Decimal), ("places", P::Places), ("rounding", P::Rounding)], 3, Ret::Decimal,
        "Rounds a decimal to the places given."),
    function(FnId::Div, "div", "numbers",
        &[("a", P::Decimal), ("b", P::Decimal), ("places", P::Places), ("rounding", P::Rounding)], 4, Ret::Decimal,
        "The quotient of two decimals, correctly rounded. There is no division without places and a rounding mode."),
    function(FnId::ParseDate, "parse_date", "dates", &[("text", P::Text), ("format", P::DateFormat)], 2, Ret::Date,
        "Reads a date written in the format, such as \"MM/DD/YYYY\"."),
    function(FnId::FormatDate, "format_date", "dates", &[("date", P::Date), ("format", P::DateFormat)], 2, Ret::Text,
        "Writes a date in the format."),
    function(FnId::ParseDatetime, "parse_datetime", "dates",
        &[("text", P::Text), ("format", P::DatetimeFormat), ("zone", P::Zone)], 3, Ret::Instant,
        "Reads a moment written in the format as the wall time of the zone: \"UTC\" or a fixed offset such as \"+02:00\"."),
    function(FnId::FormatDatetime, "format_datetime", "dates",
        &[("moment", P::Instant), ("format", P::DatetimeFormat), ("zone", P::Zone)], 3, Ret::Text,
        "Writes a moment in the format, as the wall time of the zone."),
    function(FnId::ParseTime, "parse_time", "dates", &[("text", P::Text), ("format", P::TimeFormat)], 2, Ret::Duration,
        "Reads a time of day written in the format, as the time since midnight."),
    function(FnId::ExcelSerialToDate, "excel_serial_to_date", "dates",
        &[("serial", P::Number), ("system", P::DateSystem)], 2, Ret::Date,
        "The date a workbook's serial number stands for, in the workbook's date system: \"1900\" or \"1904\"."),
    function(FnId::Trim, "trim", "text", &[("text", P::Text)], 1, Ret::Text,
        "The text without whitespace at either end."),
    function(FnId::Lower, "lower", "text", &[("text", P::Text)], 1, Ret::Text,
        "The text in lower case, by the rule that needs no locale."),
    function(FnId::Upper, "upper", "text", &[("text", P::Text)], 1, Ret::Text,
        "The text in upper case, by the rule that needs no locale."),
    function(FnId::Nfc, "nfc", "text", &[("text", P::Text)], 1, Ret::Text,
        "The text in Unicode normalization form C."),
    function(FnId::CollapseSpace, "collapse_space", "text", &[("text", P::Text)], 1, Ret::Text,
        "The text with every run of whitespace as one space and none at either end. No-break spaces are whitespace."),
    function(FnId::CodeFor, "code_for", "choices", &[("text", P::Text), ("codes", P::Codes)], 2, Ret::Choice,
        "The stored code for a code or a label, against the code list given. Text that is neither stops."),
    function(FnId::LabelFor, "label_for", "choices", &[("code", P::Text), ("codes", P::Codes)], 2, Ret::Text,
        "The label shown for a stored code."),
    function(FnId::Coalesce, "coalesce", "absence", &[("value", P::Any), ("default", P::Any)], 2, Ret::Same,
        "The first argument that is present. The only way to give a default."),
    function(FnId::Size, "size", "lists", &[("value", P::Any)], 1, Ret::Int,
        "The number of characters in text, items in a list, or fields in a record."),
    function(FnId::AsText, "as_text", "field types",
        &[("value", P::Any), ("case", P::Case), ("whitespace", P::Whitespace)], 3, Ret::Text,
        "Normalizes text: form C, then the whitespace rule, then the case rule."),
    function(FnId::AsInteger, "as_integer", "field types", &[("value", P::Any), ("mark", P::Mark)], 2, Ret::Int,
        "Normalizes a whole number, given as one or as displayed text."),
    function(FnId::AsDecimal, "as_decimal", "field types",
        &[("value", P::Any), ("places", P::Places), ("rounding", P::Rounding), ("mark", P::Mark)], 4, Ret::Decimal,
        "Normalizes a number, given as one or as displayed text, to the field type's places."),
    function(FnId::AsMoney, "as_money", "field types",
        &[("value", P::Any), ("currency", P::Currency), ("places", P::Places), ("rounding", P::Rounding), ("mark", P::Mark)], 5, Ret::Decimal,
        "Normalizes money, given as a decimal or as displayed text, to the currency's places."),
    function(FnId::AsMeasurement, "as_measurement", "field types",
        &[("value", P::Any), ("display_unit", P::DisplayUnit), ("unit", P::Unit), ("step", P::Decimal), ("rounding", P::Rounding), ("mark", P::Mark)], 6, Ret::Decimal,
        "Normalizes a measurement to the nearest step in the field's unit. A number is already in that unit; text is in the display unit."),
    function(FnId::AsDate, "as_date", "field types", &[("value", P::Any), ("format", P::DateFormat)], 2, Ret::Date,
        "Normalizes a date, given as one or as text in the format."),
    function(FnId::AsDatetime, "as_datetime", "field types",
        &[("value", P::Any), ("format", P::DatetimeFormat), ("zone", P::Zone)], 3, Ret::Instant,
        "Normalizes a moment, given as one or as text in the format and zone."),
    function(FnId::AsTime, "as_time", "field types", &[("value", P::Any), ("format", P::TimeFormat)], 2, Ret::Duration,
        "Normalizes a time of day, given as a time since midnight or as text in the format."),
    function(FnId::AsBoolean, "as_boolean", "field types",
        &[("value", P::Any), ("true_text", P::Label), ("false_text", P::Label)], 3, Ret::Bool,
        "Normalizes true or false, given as one or as one of the two labels."),
    function(FnId::AsChoice, "as_choice", "field types", &[("value", P::Any), ("codes", P::Codes)], 2, Ret::Choice,
        "Normalizes a choice to its stored code, given a code or a label."),
];

static LIBRARY: Library = Library {
    version: LibraryVersion::CURRENT,
    cost_limit: COST_LIMIT,
    functions: &FUNCTIONS,
};

/// The host-function library this engine carries. For `docs.get` and for the
/// type checker.
#[must_use]
pub fn library() -> &'static Library {
    &LIBRARY
}
