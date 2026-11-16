//! What the application stores, and the rules it applies before storing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Lengths are kept as whole sixteenths of an inch, so no value is ever a float.
const TEN_THOUSANDTHS_PER_SIXTEENTH: u64 = 625;
const MAX_LENGTH_SIXTEENTHS: u64 = 1200 * 16;
const MAX_QUANTITY: u64 = 9999;
const MAX_DESCRIPTION_CHARS: usize = 80;

/// Whether an order can still be changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    /// Line items can be added and changed.
    Draft,
    /// The order is closed; every write is refused.
    Submitted,
}

/// One line item exactly as stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredItem {
    /// The item's code, unique within its order.
    pub item_code: String,
    /// Free text.
    pub description: String,
    /// How many.
    pub quantity: u32,
    /// The length in whole sixteenths of an inch.
    pub length_sixteenths: u32,
    /// Starts at 1 and goes up by one on every stored change.
    pub version: u64,
}

/// One order exactly as stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOrder {
    /// The order's identifier.
    pub order_id: String,
    /// Who the order is for.
    pub customer: String,
    /// Whether it can still be changed.
    pub status: OrderStatus,
    /// Its line items, by item code.
    pub items: BTreeMap<String, StoredItem>,
}

/// What a write did to a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteAction {
    /// The record did not exist before.
    Created,
    /// The record existed and was changed.
    Updated,
}

/// One stored write: which request changed which record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteRecord {
    /// The number of the request that made the write, counted since the last reset.
    pub request: u64,
    /// The order written to.
    pub order_id: String,
    /// The line item written.
    pub item_code: String,
    /// Whether it was created or updated.
    pub action: WriteAction,
}

/// The values a caller asks to store for a line item.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemInput {
    /// Free text.
    pub description: String,
    /// How many.
    pub quantity: i64,
    /// The length in inches, as a decimal in text, such as `150.5`.
    pub length_in: String,
    /// The version the caller last read. Required when the item exists.
    #[serde(default)]
    pub version: Option<u64>,
}

/// One problem with one field of an input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldProblem {
    /// The field at fault.
    pub field: String,
    /// A sentence a person can act on.
    pub message: String,
}

/// Why a write was refused. Nothing is stored when one of these is returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteRefused {
    /// No order has that identifier.
    NoSuchOrder,
    /// The order is no longer a draft.
    OrderNotDraft,
    /// One or more fields are not acceptable.
    Invalid(Vec<FieldProblem>),
    /// The caller's version is not the stored one: someone else changed the record.
    VersionConflict {
        /// The version now stored.
        current: u64,
    },
}

/// Everything the application holds.
#[derive(Debug)]
pub struct Store {
    orders: BTreeMap<String, StoredOrder>,
    writes: Vec<WriteRecord>,
    requests: u64,
    stock_calls: u64,
}

impl Default for Store {
    fn default() -> Self {
        Self::seeded()
    }
}

impl Store {
    /// The known data every reset restores.
    #[must_use]
    pub fn seeded() -> Self {
        let item = |code: &str, description: &str, quantity: u32, length_sixteenths: u32| {
            (
                code.to_owned(),
                StoredItem {
                    item_code: code.to_owned(),
                    description: description.to_owned(),
                    quantity,
                    length_sixteenths,
                    version: 1,
                },
            )
        };
        let order =
            |id: &str, customer: &str, status: OrderStatus, items: Vec<(String, StoredItem)>| {
                (
                    id.to_owned(),
                    StoredOrder {
                        order_id: id.to_owned(),
                        customer: customer.to_owned(),
                        status,
                        items: items.into_iter().collect(),
                    },
                )
            };
        let orders = [
            order(
                "A-100",
                "Example Customer One",
                OrderStatus::Draft,
                vec![
                    item("L-101", "Left rail", 2, 2408),
                    item("L-102", "Right rail", 2, 2408),
                    item("L-103", "Cross brace", 4, 577),
                ],
            ),
            order(
                "A-101",
                "Example Customer Two",
                OrderStatus::Submitted,
                vec![item("L-201", "Base plate", 1, 384)],
            ),
            order(
                "A-102",
                "Example Customer Three",
                OrderStatus::Draft,
                vec![],
            ),
        ];
        Self {
            orders: orders.into_iter().collect(),
            writes: Vec::new(),
            requests: 0,
            stock_calls: 0,
        }
    }

    /// Every order, in identifier order.
    pub fn orders(&self) -> impl Iterator<Item = &StoredOrder> {
        self.orders.values()
    }

    /// One order, if it exists.
    #[must_use]
    pub fn order(&self, order_id: &str) -> Option<&StoredOrder> {
        self.orders.get(order_id)
    }

    /// One line item, if it exists.
    #[must_use]
    pub fn item(&self, order_id: &str, item_code: &str) -> Option<&StoredItem> {
        self.orders.get(order_id)?.items.get(item_code)
    }

    /// Every write since the last reset, oldest first.
    #[must_use]
    pub fn writes(&self) -> &[WriteRecord] {
        &self.writes
    }

    /// Counts a request to the stock endpoint and says whether this one fails.
    /// Every third call fails, counted since the last reset.
    pub fn stock_call_fails(&mut self) -> bool {
        self.stock_calls += 1;
        self.stock_calls.is_multiple_of(3)
    }

    /// Creates or updates one line item, or refuses and stores nothing.
    ///
    /// # Errors
    ///
    /// Returns why the write was refused.
    pub fn put_item(
        &mut self,
        order_id: &str,
        item_code: &str,
        input: &ItemInput,
    ) -> Result<(StoredItem, WriteAction), WriteRefused> {
        self.requests += 1;
        let request = self.requests;
        let order = self
            .orders
            .get_mut(order_id)
            .ok_or(WriteRefused::NoSuchOrder)?;
        if order.status != OrderStatus::Draft {
            return Err(WriteRefused::OrderNotDraft);
        }
        let existing = order.items.get(item_code);
        let (quantity, length_sixteenths) = validate(item_code, input, existing.is_some())?;
        let (version, action) = match existing {
            Some(current) if input.version != Some(current.version) => {
                return Err(WriteRefused::VersionConflict {
                    current: current.version,
                });
            }
            Some(current) => (current.version + 1, WriteAction::Updated),
            None => (1, WriteAction::Created),
        };
        let stored = StoredItem {
            item_code: item_code.to_owned(),
            description: input.description.trim().to_owned(),
            quantity,
            length_sixteenths,
            version,
        };
        order.items.insert(item_code.to_owned(), stored.clone());
        self.writes.push(WriteRecord {
            request,
            order_id: order_id.to_owned(),
            item_code: item_code.to_owned(),
            action,
        });
        Ok((stored, action))
    }
}

/// Checks every field and reports every problem at once.
fn validate(item_code: &str, input: &ItemInput, exists: bool) -> Result<(u32, u32), WriteRefused> {
    let mut problems = Vec::new();
    let mut problem = |field: &str, message: &str| {
        problems.push(FieldProblem {
            field: field.to_owned(),
            message: message.to_owned(),
        });
    };

    let code_ok = (1..=20).contains(&item_code.len())
        && item_code
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-');
    if !code_ok {
        problem(
            "item_code",
            "Item code must be 1 to 20 capital letters, digits, or hyphens.",
        );
    }
    let description = input.description.trim();
    if description.is_empty() {
        problem("description", "Description is required.");
    } else if description.chars().count() > MAX_DESCRIPTION_CHARS {
        problem("description", "Description must be 80 characters or fewer.");
    }
    let quantity = u64::try_from(input.quantity)
        .ok()
        .filter(|q| (1..=MAX_QUANTITY).contains(q));
    if quantity.is_none() {
        problem("quantity", "Quantity must be between 1 and 9999.");
    }
    let length = match parse_length_in(&input.length_in) {
        Ok(sixteenths) => Some(sixteenths),
        Err(message) => {
            problem("length_in", message);
            None
        }
    };
    if exists && input.version.is_none() {
        problem(
            "version",
            "Version is required when the line item already exists.",
        );
    }
    if !exists && input.version.is_some() {
        problem(
            "version",
            "Version must be left out when the line item does not exist yet.",
        );
    }

    match (quantity.and_then(|q| u32::try_from(q).ok()), length) {
        (Some(quantity), Some(length)) if problems.is_empty() => Ok((quantity, length)),
        _ => Err(WriteRefused::Invalid(problems)),
    }
}

/// Reads a length in inches written as a decimal, such as `150.5`, into sixteenths.
///
/// # Errors
///
/// Returns a sentence for the person who typed the value.
pub fn parse_length_in(text: &str) -> Result<u32, &'static str> {
    const NOT_A_NUMBER: &str = "Length must be a number of inches, such as 150.5.";
    const NOT_A_SIXTEENTH: &str = "Length must be a multiple of 1/16 inch.";
    const OUT_OF_RANGE: &str = "Length must be more than 0 and at most 1200 inches.";

    let text = text.trim();
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits_only = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if whole.is_empty() || whole.len() > 6 || !digits_only(whole) || !digits_only(fraction) {
        return Err(NOT_A_NUMBER);
    }
    if text.ends_with('.') {
        return Err(NOT_A_NUMBER);
    }
    if fraction.len() > 4 {
        return Err(NOT_A_SIXTEENTH);
    }
    let whole: u64 = whole.parse().map_err(|_| NOT_A_NUMBER)?;
    let fraction: u64 = format!("{fraction:0<4}")
        .parse()
        .map_err(|_| NOT_A_NUMBER)?;
    let ten_thousandths = whole * 10_000 + fraction;
    if !ten_thousandths.is_multiple_of(TEN_THOUSANDTHS_PER_SIXTEENTH) {
        return Err(NOT_A_SIXTEENTH);
    }
    let sixteenths = ten_thousandths / TEN_THOUSANDTHS_PER_SIXTEENTH;
    if sixteenths == 0 || sixteenths > MAX_LENGTH_SIXTEENTHS {
        return Err(OUT_OF_RANGE);
    }
    u32::try_from(sixteenths).map_err(|_| OUT_OF_RANGE)
}

/// Writes sixteenths of an inch as a decimal number of inches, such as `150.5`.
#[must_use]
pub fn format_length_in(sixteenths: u32) -> String {
    let ten_thousandths = u64::from(sixteenths) * TEN_THOUSANDTHS_PER_SIXTEENTH;
    let (whole, fraction) = (ten_thousandths / 10_000, ten_thousandths % 10_000);
    if fraction == 0 {
        return whole.to_string();
    }
    let fraction = format!("{fraction:04}");
    format!("{whole}.{}", fraction.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_are_read_exactly() {
        assert_eq!(parse_length_in("150.5"), Ok(2408));
        assert_eq!(parse_length_in("36.0625"), Ok(577));
        assert_eq!(parse_length_in("12"), Ok(192));
        assert_eq!(parse_length_in(" 0.0625 "), Ok(1));
        assert_eq!(parse_length_in("1200"), Ok(19200));
    }

    #[test]
    fn lengths_that_are_not_sixteenths_are_refused() {
        assert_eq!(
            parse_length_in("150.3"),
            Err("Length must be a multiple of 1/16 inch.")
        );
        assert_eq!(
            parse_length_in("1.03125"),
            Err("Length must be a multiple of 1/16 inch.")
        );
    }

    #[test]
    fn lengths_that_are_not_numbers_or_out_of_range_are_refused() {
        for text in ["", "abc", "-5", "1e3", "12.", ".5", "1,5", "1234567"] {
            assert_eq!(
                parse_length_in(text),
                Err("Length must be a number of inches, such as 150.5."),
                "{text:?}"
            );
        }
        for text in ["0", "0.0", "1200.0625", "999999"] {
            assert_eq!(
                parse_length_in(text),
                Err("Length must be more than 0 and at most 1200 inches."),
                "{text:?}"
            );
        }
    }

    #[test]
    fn lengths_are_written_back_the_way_they_were_read() {
        for text in ["150.5", "36.0625", "12", "0.0625", "1200"] {
            let sixteenths = parse_length_in(text).unwrap();
            assert_eq!(format_length_in(sixteenths), text);
        }
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let mut store = Store::seeded();
        let input = ItemInput {
            description: "  ".to_owned(),
            quantity: 0,
            length_in: "x".to_owned(),
            version: None,
        };
        let Err(WriteRefused::Invalid(problems)) = store.put_item("A-100", "L-101", &input) else {
            panic!("the write should have been refused as invalid");
        };
        let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
        assert_eq!(fields, ["description", "quantity", "length_in", "version"]);
        assert!(store.writes().is_empty());
    }
}
