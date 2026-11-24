//! Between a register's raw bits and the typed value a caller sees.
//!
//! A raw number becomes a decimal by its scale, exactly. A value to write
//! is checked against the register's safe range twice before any frame is
//! built: as the value in the register's unit, and as the raw number it
//! encodes to. Nothing is rounded here: a value that is not a whole number
//! of the register's steps is refused.

use a2m_core::{Decimal, Value};

use crate::link::{Reply, Request};
use crate::map::{CompiledRegister, Data, ValueKind, WordOrder, raw_of};

/// Why a value cannot be written to a register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unwritable {
    /// The value is not of the kind the register takes.
    WrongKind,
    /// The value is outside the register's safe range, or the register
    /// declares none.
    OutOfRange,
    /// The value is not a whole number of the register's steps, or is more
    /// than the register's bits can hold.
    NotRepresentable,
}

/// The request that reads a register.
pub(crate) const fn read_request(register: &CompiledRegister) -> Request {
    if register.table.holds_bits() {
        Request::ReadBits {
            table: register.table,
            offset: register.offset,
            count: 1,
        }
    } else {
        Request::ReadWords {
            table: register.table,
            offset: register.offset,
            count: register.words,
        }
    }
}

/// The raw number in a register's words.
fn raw_number(register: &CompiledRegister, words: &[u16]) -> Option<i64> {
    let two = |first: u16, second: u16| {
        let (high, low) = match register.word_order {
            WordOrder::HighFirst => (first, second),
            WordOrder::LowFirst => (second, first),
        };
        u32::from(high) << 16 | u32::from(low)
    };
    match (register.data, words) {
        (Data::U16, [word]) => Some(i64::from(*word)),
        (Data::I16, [word]) => Some(i64::from(i16::from_be_bytes(word.to_be_bytes()))),
        (Data::U32, [first, second]) => Some(i64::from(two(*first, *second))),
        (Data::I32, [first, second]) => Some(i64::from(i32::from_be_bytes(
            two(*first, *second).to_be_bytes(),
        ))),
        _ => None,
    }
}

/// The value a reply holds for a register, or `None` when the reply is not
/// of the form the register's layout declares.
pub(crate) fn decode(register: &CompiledRegister, reply: &Reply) -> Option<Value> {
    match (register.data, reply) {
        (Data::Bit, Reply::Bits(bits)) => {
            let [bit] = bits.as_slice() else {
                return None;
            };
            match register.kind {
                ValueKind::Bool => Some(Value::Bool(*bit)),
                ValueKind::Code => Some(Value::Text(u8::from(*bit).to_string())),
                _ => None,
            }
        }
        (Data::Text, Reply::Words(words)) => {
            if words.len() != usize::from(register.words) {
                return None;
            }
            let mut bytes: Vec<u8> = words.iter().flat_map(|word| word.to_be_bytes()).collect();
            while bytes.last() == Some(&0) {
                bytes.pop();
            }
            let text = String::from_utf8(bytes).ok()?;
            (!text.chars().any(char::is_control)).then_some(Value::Text(text))
        }
        (_, Reply::Words(words)) => {
            let raw = raw_number(register, words)?;
            match register.kind {
                ValueKind::Int => Some(Value::Int(raw)),
                ValueKind::Code => Some(Value::Text(raw.to_string())),
                ValueKind::Decimal => {
                    let raw = Decimal::from_i64(raw);
                    match &register.scale {
                        None => Some(Value::Decimal(raw)),
                        Some(scale) => raw.checked_mul(scale).map(Value::Decimal),
                    }
                }
                ValueKind::Bool | ValueKind::Text => None,
            }
        }
        _ => None,
    }
}

/// The request that writes `value` to a register, after both range checks.
pub(crate) fn write_request(
    register: &CompiledRegister,
    value: &Value,
) -> Result<Request, Unwritable> {
    if register.data == Data::Bit {
        let on = match (register.kind, value) {
            (ValueKind::Bool, Value::Bool(on)) => *on,
            (ValueKind::Code, Value::Text(code)) if code == "1" => true,
            (ValueKind::Code, Value::Text(code)) if code == "0" => false,
            (ValueKind::Code, Value::Text(_)) => return Err(Unwritable::OutOfRange),
            _ => return Err(Unwritable::WrongKind),
        };
        return Ok(Request::WriteBit {
            offset: register.offset,
            on,
        });
    }
    let (least, most) = register.data.bounds().ok_or(Unwritable::WrongKind)?;
    let in_unit = match (register.kind, value) {
        (ValueKind::Int, Value::Int(number)) => Decimal::from_i64(*number),
        (ValueKind::Decimal, Value::Decimal(number)) => *number,
        (ValueKind::Code, Value::Text(code)) => code
            .parse::<i64>()
            .map(Decimal::from_i64)
            .map_err(|_| Unwritable::OutOfRange)?,
        _ => return Err(Unwritable::WrongKind),
    };
    // A number is written only inside a declared range. The compiler
    // refuses a map without one; this holds even if it did not.
    let range = register.range.as_ref().ok_or(Unwritable::OutOfRange)?;
    // First: the value, in the register's unit.
    if in_unit < range.min || in_unit > range.max {
        return Err(Unwritable::OutOfRange);
    }
    let raw = raw_of(&in_unit, register.scale.as_ref()).ok_or(Unwritable::NotRepresentable)?;
    // Second: the raw number that will be sent, against the same range
    // converted by the scale, and against what the bits can hold.
    if raw < range.raw_min || raw > range.raw_max {
        return Err(Unwritable::OutOfRange);
    }
    if raw < least || raw > most {
        return Err(Unwritable::NotRepresentable);
    }
    let words = match register.data {
        Data::U16 => vec![u16::try_from(raw).map_err(|_| Unwritable::NotRepresentable)?],
        Data::I16 => {
            let signed = i16::try_from(raw).map_err(|_| Unwritable::NotRepresentable)?;
            vec![u16::from_be_bytes(signed.to_be_bytes())]
        }
        Data::U32 | Data::I32 => {
            let bits = if register.data == Data::U32 {
                u32::try_from(raw).map_err(|_| Unwritable::NotRepresentable)?
            } else {
                let signed = i32::try_from(raw).map_err(|_| Unwritable::NotRepresentable)?;
                u32::from_be_bytes(signed.to_be_bytes())
            };
            let [a, b, c, d] = bits.to_be_bytes();
            let (high, low) = (u16::from_be_bytes([a, b]), u16::from_be_bytes([c, d]));
            match register.word_order {
                WordOrder::HighFirst => vec![high, low],
                WordOrder::LowFirst => vec![low, high],
            }
        }
        Data::Bit | Data::Text => return Err(Unwritable::WrongKind),
    };
    Ok(Request::WriteWords {
        offset: register.offset,
        words,
    })
}

#[cfg(test)]
mod tests {
    use a2m_core::{Decimal, Value};

    use super::{Unwritable, decode, write_request};
    use crate::link::{Reply, Request};
    use crate::map::{
        Access, CompiledRange, CompiledRegister, Data, ReadEffect, Table, ValueKind, WordOrder,
    };

    fn decimal(text: &str) -> Decimal {
        Decimal::parse(text).unwrap()
    }

    fn register(
        data: Data,
        scale: Option<&str>,
        range: Option<(&str, &str, i64, i64)>,
    ) -> CompiledRegister {
        CompiledRegister {
            table: Table::Holding,
            offset: 100,
            data,
            words: data.words().unwrap_or(1),
            word_order: WordOrder::HighFirst,
            scale: scale.map(decimal),
            kind: if scale.is_some() {
                ValueKind::Decimal
            } else {
                ValueKind::Int
            },
            access: Access::ReadWrite,
            range: range.map(|(min, max, raw_min, raw_max)| CompiledRange {
                min: decimal(min),
                max: decimal(max),
                raw_min,
                raw_max,
            }),
            hazard: None,
            read_effect: ReadEffect::None,
            confirm_by: None,
        }
    }

    #[test]
    fn a_raw_number_becomes_a_decimal_by_its_scale_exactly() {
        let speed = register(Data::U16, Some("0.1"), None);
        assert_eq!(
            decode(&speed, &Reply::Words(vec![12_345])),
            Some(Value::Decimal(decimal("1234.5")))
        );
        let offset = register(Data::I16, Some("0.1"), None);
        assert_eq!(
            decode(&offset, &Reply::Words(vec![0xFFDD])),
            Some(Value::Decimal(decimal("-3.5")))
        );
        let mut low_first = register(Data::U32, None, None);
        low_first.word_order = WordOrder::LowFirst;
        assert_eq!(
            decode(&low_first, &Reply::Words(vec![0x4240, 0x000F])),
            Some(Value::Int(1_000_000))
        );
        // A reply of another shape is not a value.
        assert_eq!(decode(&speed, &Reply::Words(vec![1, 2])), None);
        assert_eq!(decode(&speed, &Reply::Bits(vec![true])), None);
    }

    #[test]
    fn a_value_outside_its_range_gives_no_request() {
        let speed = register(Data::U16, Some("0.1"), Some(("0", "1450", 0, 14_500)));
        let write = |text: &str| write_request(&speed, &Value::Decimal(decimal(text)));
        assert_eq!(
            write("1200"),
            Ok(Request::WriteWords {
                offset: 100,
                words: vec![12_000]
            })
        );
        assert_eq!(write("1450"), write("1450.0"));
        assert_eq!(write("1450.1"), Err(Unwritable::OutOfRange));
        assert_eq!(write("-0.1"), Err(Unwritable::OutOfRange));
        // Inside the range and between two steps: refused, never rounded.
        assert_eq!(write("1200.05"), Err(Unwritable::NotRepresentable));
        // A whole number is not a decimal, and nothing is converted.
        assert_eq!(
            write_request(&speed, &Value::Int(1200)),
            Err(Unwritable::WrongKind)
        );
        // No range, no write.
        let unranged = register(Data::U16, Some("0.1"), None);
        assert_eq!(
            write_request(&unranged, &Value::Decimal(decimal("1"))),
            Err(Unwritable::OutOfRange)
        );
        // The raw check holds on its own: a range whose raw bounds are
        // tighter than its decimal bounds still stops the value.
        let tight = register(Data::U16, Some("0.1"), Some(("0", "1450", 0, 100)));
        assert_eq!(
            write_request(&tight, &Value::Decimal(decimal("1200"))),
            Err(Unwritable::OutOfRange)
        );
    }

    #[test]
    fn signed_and_two_word_values_are_laid_out_by_the_map() {
        let offset = register(Data::I16, Some("0.1"), Some(("-10", "10", -100, 100)));
        assert_eq!(
            write_request(&offset, &Value::Decimal(decimal("-3.5"))),
            Ok(Request::WriteWords {
                offset: 100,
                words: vec![0xFFDD]
            })
        );
        let mut batch = register(Data::U32, None, Some(("0", "1000000", 0, 1_000_000)));
        batch.word_order = WordOrder::LowFirst;
        assert_eq!(
            write_request(&batch, &Value::Int(1_000_000)),
            Ok(Request::WriteWords {
                offset: 100,
                words: vec![0x4240, 0x000F]
            })
        );
    }
}
