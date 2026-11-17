//! The frames of the Modbus application protocol over TCP that this stand speaks.
//!
//! A frame is a seven-byte MBAP header followed by a PDU: a function code and
//! its data. The frames are written here, with no protocol library, so the
//! stand does not share code with the client it is used to test.

/// Bytes in the MBAP header.
pub const HEADER_BYTES: usize = 7;
/// The largest PDU the protocol allows.
pub const MAX_PDU_BYTES: usize = 253;
/// The unit identifier that means "the device at this address" over TCP.
pub const UNIT_NOT_USED: u8 = 0xFF;

/// Function code 01, Read Coils.
pub const READ_COILS: u8 = 0x01;
/// Function code 02, Read Discrete Inputs.
pub const READ_DISCRETE_INPUTS: u8 = 0x02;
/// Function code 03, Read Holding Registers.
pub const READ_HOLDING_REGISTERS: u8 = 0x03;
/// Function code 04, Read Input Registers.
pub const READ_INPUT_REGISTERS: u8 = 0x04;
/// Function code 05, Write Single Coil.
pub const WRITE_SINGLE_COIL: u8 = 0x05;
/// Function code 06, Write Single Register.
pub const WRITE_SINGLE_REGISTER: u8 = 0x06;
/// Function code 16, Write Multiple Registers.
pub const WRITE_MULTIPLE_REGISTERS: u8 = 0x10;
/// Function code 43, Encapsulated Interface Transport.
pub const ENCAPSULATED_INTERFACE: u8 = 0x2B;
/// MEI type 14 of function code 43, Read Device Identification.
pub const MEI_READ_DEVICE_IDENTIFICATION: u8 = 0x0E;

/// The value that switches a coil on in Write Single Coil.
pub const COIL_ON: u16 = 0xFF00;
/// The value that switches a coil off in Write Single Coil.
pub const COIL_OFF: u16 = 0x0000;

/// The exception codes this stand answers with, by their standard names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exception {
    /// 01: the function code is not one the device supports.
    IllegalFunction,
    /// 02: an address in the request is not one the device offers for that function.
    IllegalDataAddress,
    /// 03: a value in the request is not one the device will take.
    IllegalDataValue,
    /// 04: the device could not carry the request out in its present state.
    ServerDeviceFailure,
    /// 06: the device is busy and the request should be sent again later.
    ServerDeviceBusy,
    /// 0B: no device answers for the unit identifier in the request.
    GatewayTargetFailedToRespond,
}

impl Exception {
    /// The code as it travels in an exception reply.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::IllegalFunction => 0x01,
            Self::IllegalDataAddress => 0x02,
            Self::IllegalDataValue => 0x03,
            Self::ServerDeviceFailure => 0x04,
            Self::ServerDeviceBusy => 0x06,
            Self::GatewayTargetFailedToRespond => 0x0B,
        }
    }
}

/// The MBAP header of one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Chosen by the client and copied into the reply.
    pub transaction: u16,
    /// Always zero for this protocol.
    pub protocol: u16,
    /// The number of bytes that follow this field: the unit identifier and the PDU.
    pub length: u16,
    /// The unit identifier.
    pub unit: u8,
}

impl Header {
    /// Reads a header from its seven bytes.
    #[must_use]
    pub fn parse(bytes: &[u8; HEADER_BYTES]) -> Self {
        Self {
            transaction: u16::from_be_bytes([bytes[0], bytes[1]]),
            protocol: u16::from_be_bytes([bytes[2], bytes[3]]),
            length: u16::from_be_bytes([bytes[4], bytes[5]]),
            unit: bytes[6],
        }
    }

    /// How many PDU bytes follow, or `None` for a header that is not this protocol's.
    #[must_use]
    pub fn pdu_bytes(&self) -> Option<usize> {
        let following = usize::from(self.length);
        (self.protocol == 0 && (2..=MAX_PDU_BYTES + 1).contains(&following)).then(|| following - 1)
    }
}

/// Whether a frame asks the device for something or changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    /// A function that only reads: 01, 02, 03, 04, and 43.
    Read,
    /// A function that writes: 05, 06, and 16, and the standard write functions
    /// this stand refuses (15, 21, 22, 23), so that a refused attempt still counts.
    Write,
    /// Anything else, and a frame whose header was not this protocol's.
    Other,
}

/// Classifies a function code.
#[must_use]
pub const fn kind_of(function: u8) -> FrameKind {
    match function {
        READ_COILS
        | READ_DISCRETE_INPUTS
        | READ_HOLDING_REGISTERS
        | READ_INPUT_REGISTERS
        | ENCAPSULATED_INTERFACE => FrameKind::Read,
        WRITE_SINGLE_COIL
        | WRITE_SINGLE_REGISTER
        | WRITE_MULTIPLE_REGISTERS
        | 0x0F
        | 0x15
        | 0x16
        | 0x17 => FrameKind::Write,
        _ => FrameKind::Other,
    }
}

/// One request, decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Function 01.
    ReadCoils {
        /// The first coil.
        address: u16,
        /// How many coils.
        quantity: u16,
    },
    /// Function 02.
    ReadDiscreteInputs {
        /// The first input.
        address: u16,
        /// How many inputs.
        quantity: u16,
    },
    /// Function 03.
    ReadHoldingRegisters {
        /// The first register.
        address: u16,
        /// How many registers.
        quantity: u16,
    },
    /// Function 04.
    ReadInputRegisters {
        /// The first register.
        address: u16,
        /// How many registers.
        quantity: u16,
    },
    /// Function 05.
    WriteSingleCoil {
        /// The coil.
        address: u16,
        /// The value as sent: `0xFF00` for on, `0x0000` for off.
        value: u16,
    },
    /// Function 06.
    WriteSingleRegister {
        /// The register.
        address: u16,
        /// The value as sent.
        value: u16,
    },
    /// Function 16.
    WriteMultipleRegisters {
        /// The first register.
        address: u16,
        /// The values as sent, one per register.
        values: Vec<u16>,
    },
    /// Function 43 with MEI type 14.
    ReadDeviceIdentification {
        /// The read code: 1 basic, 2 regular, 3 extended, 4 one object.
        code: u8,
        /// The object to start at, or the one object asked for.
        object: u8,
    },
    /// A function code this stand does not carry.
    Unsupported {
        /// The function code.
        function: u8,
    },
    /// A function this stand carries, with data of the wrong length.
    Malformed {
        /// The function code.
        function: u8,
    },
    /// A header that is not this protocol's. The link is closed without a reply.
    BadHeader,
}

fn word(data: &[u8], at: usize) -> u16 {
    let high = data.get(at).copied().unwrap_or(0);
    let low = data.get(at + 1).copied().unwrap_or(0);
    u16::from_be_bytes([high, low])
}

/// Decodes a PDU. The second value is the exception the protocol itself calls
/// for, before the device is asked: an unknown function, or a count outside
/// the standard's limits.
#[must_use]
pub fn decode(pdu: &[u8]) -> (Request, Option<Exception>) {
    let Some((&function, data)) = pdu.split_first() else {
        return (
            Request::Unsupported { function: 0 },
            Some(Exception::IllegalFunction),
        );
    };
    let malformed = (
        Request::Malformed { function },
        Some(Exception::IllegalDataValue),
    );
    match function {
        READ_COILS | READ_DISCRETE_INPUTS | READ_HOLDING_REGISTERS | READ_INPUT_REGISTERS => {
            if data.len() != 4 {
                return malformed;
            }
            let (address, quantity) = (word(data, 0), word(data, 2));
            let most = if function <= READ_DISCRETE_INPUTS {
                2000
            } else {
                125
            };
            let refused = (quantity == 0 || quantity > most).then_some(Exception::IllegalDataValue);
            let request = match function {
                READ_COILS => Request::ReadCoils { address, quantity },
                READ_DISCRETE_INPUTS => Request::ReadDiscreteInputs { address, quantity },
                READ_HOLDING_REGISTERS => Request::ReadHoldingRegisters { address, quantity },
                _ => Request::ReadInputRegisters { address, quantity },
            };
            (request, refused)
        }
        WRITE_SINGLE_COIL | WRITE_SINGLE_REGISTER => {
            if data.len() != 4 {
                return malformed;
            }
            let (address, value) = (word(data, 0), word(data, 2));
            if function == WRITE_SINGLE_COIL {
                let refused =
                    (value != COIL_ON && value != COIL_OFF).then_some(Exception::IllegalDataValue);
                (Request::WriteSingleCoil { address, value }, refused)
            } else {
                (Request::WriteSingleRegister { address, value }, None)
            }
        }
        WRITE_MULTIPLE_REGISTERS => {
            if data.len() < 5 {
                return malformed;
            }
            let (address, quantity) = (word(data, 0), usize::from(word(data, 2)));
            let (count, payload) = (usize::from(data[4]), &data[5..]);
            if !(1..=123).contains(&quantity) || count != quantity * 2 || payload.len() != count {
                return malformed;
            }
            let values = payload
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_be_bytes(*pair))
                .collect();
            (Request::WriteMultipleRegisters { address, values }, None)
        }
        ENCAPSULATED_INTERFACE => match data {
            [MEI_READ_DEVICE_IDENTIFICATION, code, object] => (
                Request::ReadDeviceIdentification {
                    code: *code,
                    object: *object,
                },
                None,
            ),
            [MEI_READ_DEVICE_IDENTIFICATION, ..] => malformed,
            _ => (
                Request::Unsupported { function },
                Some(Exception::IllegalFunction),
            ),
        },
        _ => (
            Request::Unsupported { function },
            Some(Exception::IllegalFunction),
        ),
    }
}

/// The reply to a read of bits: a byte count, then the bits, lowest bit first.
#[must_use]
pub(crate) fn bits_reply(function: u8, bits: &[bool]) -> Vec<u8> {
    let mut bytes = vec![0u8; bits.len().div_ceil(8)];
    for (i, _) in bits.iter().enumerate().filter(|(_, on)| **on) {
        bytes[i / 8] |= 1 << (i % 8);
    }
    let mut pdu = vec![function, bytes.len() as u8];
    pdu.extend(bytes);
    pdu
}

/// The reply to a read of registers: a byte count, then each word, high byte first.
#[must_use]
pub(crate) fn words_reply(function: u8, words: &[u16]) -> Vec<u8> {
    let mut pdu = vec![function, (words.len() * 2) as u8];
    pdu.extend(words.iter().flat_map(|w| w.to_be_bytes()));
    pdu
}

/// The reply to a write: the function, the address, and the value or the count.
#[must_use]
pub(crate) fn write_reply(function: u8, address: u16, second: u16) -> Vec<u8> {
    let mut pdu = vec![function];
    pdu.extend(address.to_be_bytes());
    pdu.extend(second.to_be_bytes());
    pdu
}

/// An exception reply: the function code with its top bit set, then the code.
#[must_use]
pub(crate) fn exception_reply(function: u8, code: u8) -> Vec<u8> {
    vec![function | 0x80, code]
}

/// A whole frame: the header for this PDU, then the PDU.
#[must_use]
pub(crate) fn frame(transaction: u16, unit: u8, pdu: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_BYTES + pdu.len());
    bytes.extend(transaction.to_be_bytes());
    bytes.extend(0u16.to_be_bytes());
    bytes.extend(((pdu.len() + 1) as u16).to_be_bytes());
    bytes.push(unit);
    bytes.extend(pdu);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_that_is_not_this_protocols_has_no_pdu() {
        let good = Header::parse(&[0, 7, 0, 0, 0, 6, 1]);
        assert_eq!(
            (good.transaction, good.unit, good.pdu_bytes()),
            (7, 1, Some(5))
        );
        assert_eq!(Header::parse(&[0, 7, 0, 1, 0, 6, 1]).pdu_bytes(), None);
        assert_eq!(Header::parse(&[0, 7, 0, 0, 0, 1, 1]).pdu_bytes(), None);
        assert_eq!(Header::parse(&[0, 7, 0, 0, 0, 255, 1]).pdu_bytes(), None);
    }

    #[test]
    fn counts_outside_the_standards_limits_are_illegal_values() {
        let (request, refused) = decode(&[0x03, 0, 100, 0, 126]);
        assert_eq!(
            request,
            Request::ReadHoldingRegisters {
                address: 100,
                quantity: 126
            }
        );
        assert_eq!(refused, Some(Exception::IllegalDataValue));
        assert_eq!(decode(&[0x01, 0, 0, 0x07, 0xD0]).1, None);
        assert_eq!(
            decode(&[0x05, 0, 4, 0x12, 0x34]).1,
            Some(Exception::IllegalDataValue)
        );
        let (request, refused) = decode(&[0x10, 0, 100, 0, 2, 4, 0, 1, 0, 2]);
        assert_eq!(
            request,
            Request::WriteMultipleRegisters {
                address: 100,
                values: vec![1, 2]
            }
        );
        assert_eq!(refused, None);
        assert_eq!(
            decode(&[0x10, 0, 100, 0, 2, 3, 0, 1, 0]).0,
            Request::Malformed { function: 0x10 }
        );
        assert_eq!(decode(&[0x0F, 0, 0]).1, Some(Exception::IllegalFunction));
    }

    #[test]
    fn replies_are_laid_out_as_the_standard_says() {
        assert_eq!(
            bits_reply(
                1,
                &[true, false, true, false, false, false, false, false, true]
            ),
            [1, 2, 0b101, 1]
        );
        assert_eq!(
            words_reply(3, &[0x1234, 0x00FF]),
            [3, 4, 0x12, 0x34, 0x00, 0xFF]
        );
        assert_eq!(exception_reply(6, 3), [0x86, 3]);
        assert_eq!(frame(9, 1, &[0x86, 3]), [0, 9, 0, 0, 0, 3, 1, 0x86, 3]);
    }
}
