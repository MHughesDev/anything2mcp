//! The simulated device's own tests. Each talks to a running device over its
//! link with the small client below, written here and sharing no code with the
//! device, and proves what happened by asking the control interface.

use std::error::Error;
use std::time::{Duration, Instant};

use example_device::firmware::{self, MAX_BLOCK_BYTES, MODEL_CODE};
use example_device::{
    Answer, Corruption, DeviceConfig, Fault, FaultKind, FrameKind, Request, Scope, Table,
    TestDevice, UpdateError, UpdateState, Value, Version,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

type Outcome = Result<(), Box<dyn Error>>;

/// Long enough that a wait only ends this way when something is broken.
const PATIENCE: Duration = Duration::from_secs(20);

const SETPOINT: u16 = 100;
const DOSING_RATE: u16 = 101;
const HEATER_LIMIT: u16 = 102;
const TEMPERATURE_OFFSET: u16 = 103;
const BATCH_TARGET: u16 = 104;
const RATED_SPEED: u16 = 106;
const UPDATE_COMMAND: u16 = 900;
const UPDATE_IMAGE_LENGTH: u16 = 901;
const UPDATE_BLOCK: u16 = 1000;
const VALVE: u16 = 4;

#[derive(Debug, PartialEq, Eq)]
enum Reply {
    /// The bytes after the function code of a normal reply.
    Data(Vec<u8>),
    /// The code of an exception reply.
    Exception(u8),
}

/// A client of the protocol, as small as the tests need.
struct Client {
    stream: TcpStream,
    unit: u8,
    transaction: u16,
}

fn read_pdu(function: u8, address: u16, quantity: u16) -> Vec<u8> {
    let mut pdu = vec![function];
    pdu.extend(address.to_be_bytes());
    pdu.extend(quantity.to_be_bytes());
    pdu
}

fn write_register_pdu(address: u16, value: u16) -> Vec<u8> {
    read_pdu(0x06, address, value)
}

fn write_registers_pdu(address: u16, values: &[u16]) -> Vec<u8> {
    let mut pdu = read_pdu(0x10, address, values.len() as u16);
    pdu.push((values.len() * 2) as u8);
    pdu.extend(values.iter().flat_map(|value| value.to_be_bytes()));
    pdu
}

fn write_coil_pdu(address: u16, on: bool) -> Vec<u8> {
    read_pdu(0x05, address, if on { 0xFF00 } else { 0 })
}

impl Client {
    async fn connect(device: &TestDevice) -> Result<Self, Box<dyn Error>> {
        let stream = TcpStream::connect(device.address()).await?;
        Ok(Self {
            stream,
            unit: device.unit(),
            transaction: 0,
        })
    }

    /// Sends one frame and returns its transaction identifier.
    async fn send(&mut self, pdu: &[u8]) -> Result<u16, Box<dyn Error>> {
        self.transaction += 1;
        let mut frame = Vec::new();
        frame.extend(self.transaction.to_be_bytes());
        frame.extend([0, 0]);
        frame.extend(((pdu.len() + 1) as u16).to_be_bytes());
        frame.push(self.unit);
        frame.extend(pdu);
        self.stream.write_all(&frame).await?;
        Ok(self.transaction)
    }

    /// Receives one frame: its transaction identifier and its PDU.
    async fn receive(&mut self) -> Result<(u16, Vec<u8>), Box<dyn Error>> {
        let whole = async {
            let mut head = [0u8; 7];
            self.stream.read_exact(&mut head).await?;
            let length = usize::from(u16::from_be_bytes([head[4], head[5]]));
            let mut pdu = vec![0u8; length.saturating_sub(1)];
            self.stream.read_exact(&mut pdu).await?;
            Ok::<_, std::io::Error>((u16::from_be_bytes([head[0], head[1]]), pdu))
        };
        Ok(tokio::time::timeout(PATIENCE, whole).await??)
    }

    /// Sends a request and reads its reply, checking that the reply is for it.
    async fn ask(&mut self, pdu: &[u8]) -> Result<Reply, Box<dyn Error>> {
        let sent = self.send(pdu).await?;
        let (transaction, reply) = self.receive().await?;
        if transaction != sent {
            return Err(format!("a reply for transaction {transaction}, not {sent}").into());
        }
        match reply.split_first() {
            Some((function, rest)) if *function == pdu[0] => Ok(Reply::Data(rest.to_vec())),
            Some((function, [code])) if *function == pdu[0] | 0x80 => Ok(Reply::Exception(*code)),
            _ => Err("a reply of no known form".into()),
        }
    }

    async fn data(&mut self, pdu: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        match self.ask(pdu).await? {
            Reply::Data(data) => Ok(data),
            Reply::Exception(code) => Err(format!("the device answered exception {code}").into()),
        }
    }

    async fn words(
        &mut self,
        function: u8,
        address: u16,
        quantity: u16,
    ) -> Result<Vec<u16>, Box<dyn Error>> {
        let data = self.data(&read_pdu(function, address, quantity)).await?;
        let (count, bytes) = data.split_first().ok_or("an empty reply")?;
        if usize::from(*count) != bytes.len() || bytes.len() != usize::from(quantity) * 2 {
            return Err("a reply with the wrong byte count".into());
        }
        Ok(bytes
            .chunks(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect())
    }

    async fn holding(&mut self, address: u16, quantity: u16) -> Result<Vec<u16>, Box<dyn Error>> {
        self.words(0x03, address, quantity).await
    }

    async fn input(&mut self, address: u16, quantity: u16) -> Result<Vec<u16>, Box<dyn Error>> {
        self.words(0x04, address, quantity).await
    }

    async fn bits(
        &mut self,
        function: u8,
        address: u16,
        quantity: u16,
    ) -> Result<Vec<bool>, Box<dyn Error>> {
        let data = self.data(&read_pdu(function, address, quantity)).await?;
        let bytes = data.get(1..).ok_or("an empty reply")?;
        Ok((0..usize::from(quantity))
            .map(|i| {
                bytes
                    .get(i / 8)
                    .is_some_and(|byte| byte >> (i % 8) & 1 == 1)
            })
            .collect())
    }

    /// Writes one register and checks the device echoed the request.
    async fn write(&mut self, address: u16, value: u16) -> Outcome {
        let pdu = write_register_pdu(address, value);
        let echo = self.data(&pdu).await?;
        if echo != pdu[1..] {
            return Err("the device did not echo the write".into());
        }
        Ok(())
    }

    /// Whether the device closed the link: nothing more arrives on it.
    async fn closed(&mut self) -> Result<bool, Box<dyn Error>> {
        let mut byte = [0u8; 1];
        let read = tokio::time::timeout(PATIENCE, self.stream.read(&mut byte)).await?;
        Ok(matches!(read, Ok(0) | Err(_)))
    }
}

fn number(device: &TestDevice, name: &str) -> Result<i64, Box<dyn Error>> {
    device
        .control()
        .number(name)
        .ok_or_else(|| format!("{name} is not a number point").into())
}

/// The slice's proof: write a point, then ask the device what it received and what it holds.
#[tokio::test]
async fn a_write_is_proven_by_the_frame_log_and_the_stored_state() -> Outcome {
    let device = TestDevice::start().await?;
    let mut client = Client::connect(&device).await?;
    assert_eq!(number(&device, "pump_speed_setpoint")?, 0);

    client.write(SETPOINT, 12_000).await?;

    let control = device.control();
    assert_eq!(number(&device, "pump_speed_setpoint")?, 12_000);
    assert_eq!(control.word(Table::Holding, SETPOINT), Some(12_000));
    assert_eq!(control.write_count("pump_speed_setpoint"), 1);
    let frames = control.frames();
    assert_eq!(frames.len(), 1);
    let frame = &frames[0];
    assert_eq!(
        (frame.sequence, frame.link, frame.transaction, frame.unit),
        (1, 1, 1, 1)
    );
    assert_eq!((frame.function, frame.kind), (0x06, FrameKind::Write));
    assert_eq!(
        frame.request,
        Request::WriteSingleRegister {
            address: SETPOINT,
            value: 12_000
        }
    );
    assert_eq!(frame.raw, [0, 1, 0, 0, 0, 6, 1, 0x06, 0, 100, 0x2E, 0xE0]);
    assert_eq!(
        (frame.applied, frame.answer, frame.fault),
        (true, Answer::Normal, None)
    );
    Ok(())
}

#[tokio::test]
async fn a_read_sends_only_read_frames_and_changes_nothing() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    control.set_process_frozen(true);
    let before = control.inspect().points;
    let mut client = Client::connect(&device).await?;

    assert_eq!(client.bits(0x01, VALVE, 1).await?, [false]);
    assert_eq!(client.bits(0x02, 0, 2).await?, [false, false]);
    assert_eq!(
        client.holding(SETPOINT, 7).await?,
        [0, 250, 50, 0, 5000, 0, 14_500]
    );
    assert_eq!(client.input(30, 4).await?, [0, 0xFFDD, 0, 0]);
    let identity = client.data(&[0x2B, 0x0E, 1, 0]).await?;
    assert_eq!(&identity[..6], [0x0E, 1, 0x83, 0, 0, 3]);

    let frames = control.frames();
    assert_eq!(frames.len(), 5);
    assert!(
        frames
            .iter()
            .all(|f| f.kind == FrameKind::Read && f.applied)
    );
    assert!(frames.iter().all(|f| f.answer == Answer::Normal));
    assert!(control.inspect().write_counts.is_empty());
    assert_eq!(control.inspect().points, before);
    Ok(())
}

#[tokio::test]
async fn the_device_says_what_it_is_in_registers_and_by_identification() -> Outcome {
    let device = TestDevice::start().await?;
    let mut client = Client::connect(&device).await?;

    // Version, serial number high word first, vendor and model as text.
    let words = client.input(100, 18).await?;
    assert_eq!(words[..3], [2, 4, 0]);
    assert_eq!((u32::from(words[3]) << 16) | u32::from(words[4]), 100_123);
    let text: Vec<u8> = words[5..].iter().flat_map(|w| w.to_be_bytes()).collect();
    assert_eq!(&text, b"example\0example-controller");
    assert_eq!(
        device.control().value("model"),
        Some(Value::Text("example-controller".into()))
    );

    // Basic identification as a stream: vendor, product code, revision.
    let basic = client.data(&[0x2B, 0x0E, 1, 0]).await?;
    let mut expected = vec![0x0E, 1, 0x83, 0, 0, 3, 0, 7];
    expected.extend(b"example");
    expected.extend([1, 18]);
    expected.extend(b"example-controller");
    expected.extend([2, 5]);
    expected.extend(b"2.4.0");
    assert_eq!(basic, expected);
    // Extended identification adds the serial number; one object can be asked for alone.
    let extended = client.data(&[0x2B, 0x0E, 3, 0]).await?;
    assert_eq!(extended[5], 4);
    assert!(extended.ends_with(b"\x80\x06100123"));
    let one = client.data(&[0x2B, 0x0E, 4, 2]).await?;
    assert_eq!(
        one,
        [&[0x0E, 4, 0x83, 0, 0, 1, 2, 5][..], b"2.4.0"].concat()
    );
    assert_eq!(client.ask(&[0x2B, 0x0E, 4, 9]).await?, Reply::Exception(2));
    assert_eq!(client.ask(&[0x2B, 0x0E, 7, 0]).await?, Reply::Exception(3));
    assert_eq!(client.ask(&[0x2B, 0x0D, 0, 0]).await?, Reply::Exception(1));
    Ok(())
}

#[tokio::test]
async fn values_of_each_layout_travel_as_the_point_map_says() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    let mut client = Client::connect(&device).await?;

    // Two words, low word first, written in one frame.
    let written = client
        .data(&write_registers_pdu(BATCH_TARGET, &[0x86A0, 0x0001]))
        .await?;
    assert_eq!(written, [0, 104, 0, 2]);
    assert_eq!(number(&device, "batch_target")?, 100_000);
    assert_eq!(client.holding(BATCH_TARGET, 2).await?, [0x86A0, 0x0001]);

    // A signed word; the temperature the device reports moves with it.
    client.write(TEMPERATURE_OFFSET, 0xFFCE).await?;
    assert_eq!(number(&device, "temperature_offset")?, -50);
    assert_eq!(client.input(31, 1).await?, [(-85i16) as u16]);

    // Two words, high word first, read only.
    control.set_process_frozen(true);
    control.advance(70_000);
    let uptime = client.input(32, 2).await?;
    assert_eq!((u32::from(uptime[0]) << 16) | u32::from(uptime[1]), 70_004);

    // A bit, and several points in one frame, counted once each.
    assert_eq!(
        client.ask(&write_coil_pdu(VALVE, true)).await?,
        Reply::Data(vec![0, 4, 0xFF, 0])
    );
    assert_eq!(control.bit("inlet_valve"), Some(true));
    client
        .data(&write_registers_pdu(SETPOINT, &[9000, 300, 40]))
        .await?;
    assert_eq!(number(&device, "pump_speed_setpoint")?, 9000);
    assert_eq!(number(&device, "dosing_rate")?, 300);
    assert_eq!(number(&device, "heater_power_limit")?, 40);
    assert_eq!(control.write_count("dosing_rate"), 1);
    Ok(())
}

#[tokio::test]
async fn the_device_enforces_its_own_range_and_latches_the_refusal() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    let mut client = Client::connect(&device).await?;
    client.write(SETPOINT, 14_500).await?;

    assert_eq!(
        client.ask(&write_register_pdu(SETPOINT, 14_501)).await?,
        Reply::Exception(3)
    );
    assert_eq!(number(&device, "pump_speed_setpoint")?, 14_500);
    assert_eq!(control.write_count("pump_speed_setpoint"), 1);
    let refused = &control.frames()[1];
    assert_eq!((refused.kind, refused.applied), (FrameKind::Write, false));
    assert_eq!(refused.answer, Answer::Exception(3));

    // One value out of range refuses the whole frame: nothing in it is stored.
    let frame = write_registers_pdu(SETPOINT, &[100, 10_001]);
    assert_eq!(client.ask(&frame).await?, Reply::Exception(3));
    assert_eq!(number(&device, "pump_speed_setpoint")?, 14_500);
    assert_eq!(
        client
            .ask(&write_register_pdu(TEMPERATURE_OFFSET, 101))
            .await?,
        Reply::Exception(3)
    );
    let below = (-101i16) as u16;
    assert_eq!(
        client
            .ask(&write_register_pdu(TEMPERATURE_OFFSET, below))
            .await?,
        Reply::Exception(3)
    );
    assert_eq!(
        client
            .ask(&write_registers_pdu(BATCH_TARGET, &[0x4241, 0x000F]))
            .await?,
        Reply::Exception(3)
    );

    // The latch remembers. Looking through the control interface does not
    // clear it; reading it over the link does.
    assert_eq!(number(&device, "fault_latch")?, 1);
    assert_eq!(number(&device, "fault_latch")?, 1);
    assert_eq!(client.input(7, 1).await?, [1]);
    assert_eq!(client.input(7, 1).await?, [0]);

    // The reset command clears it too, and reads as off.
    client.ask(&write_register_pdu(SETPOINT, 20_000)).await?;
    assert_eq!(number(&device, "fault_latch")?, 1);
    client.data(&write_coil_pdu(8, true)).await?;
    assert_eq!(number(&device, "fault_latch")?, 0);
    assert_eq!(client.bits(0x01, 8, 1).await?, [false]);
    assert_eq!(control.write_count("fault_reset"), 1);
    Ok(())
}

#[tokio::test]
async fn the_device_clamps_and_rounds_so_a_read_back_differs() -> Outcome {
    let device = TestDevice::start().await?;
    let mut client = Client::connect(&device).await?;

    // The echo says 95; the device holds 80.
    client.write(HEATER_LIMIT, 95).await?;
    assert_eq!(number(&device, "heater_power_limit")?, 80);
    assert_eq!(client.holding(HEATER_LIMIT, 1).await?, [80]);

    client.write(DOSING_RATE, 1237).await?;
    assert_eq!(number(&device, "dosing_rate")?, 1235);
    client.write(DOSING_RATE, 1238).await?;
    assert_eq!(client.holding(DOSING_RATE, 1).await?, [1240]);
    assert_eq!(device.control().write_count("dosing_rate"), 2);
    Ok(())
}

#[tokio::test]
async fn what_the_device_does_not_offer_is_refused_with_the_standards_exceptions() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    let mut client = Client::connect(&device).await?;

    // A read-only point in a table that can be written.
    assert_eq!(
        client.ask(&write_register_pdu(RATED_SPEED, 1)).await?,
        Reply::Exception(2)
    );
    assert_eq!(number(&device, "rated_speed")?, 14_500);
    // Half of a two-word point; an address with no point; a read that runs off the map.
    assert_eq!(
        client.ask(&write_register_pdu(BATCH_TARGET, 1)).await?,
        Reply::Exception(2)
    );
    assert_eq!(
        client.ask(&write_register_pdu(BATCH_TARGET + 1, 1)).await?,
        Reply::Exception(2)
    );
    assert_eq!(
        client.ask(&read_pdu(0x03, 50, 1)).await?,
        Reply::Exception(2)
    );
    assert_eq!(
        client.ask(&read_pdu(0x03, SETPOINT, 8)).await?,
        Reply::Exception(2)
    );
    assert_eq!(
        client.ask(&write_coil_pdu(5, true)).await?,
        Reply::Exception(2)
    );
    // Counts and values the standard does not allow.
    assert_eq!(
        client.ask(&read_pdu(0x03, SETPOINT, 0)).await?,
        Reply::Exception(3)
    );
    assert_eq!(
        client.ask(&read_pdu(0x05, VALVE, 0x1234)).await?,
        Reply::Exception(3)
    );
    // A function this device does not carry. It is a write function, and is counted as one.
    assert_eq!(
        client.ask(&[0x0F, 0, 4, 0, 1, 1, 1]).await?,
        Reply::Exception(1)
    );
    assert_eq!(client.ask(&[0x08, 0, 0, 0, 0]).await?, Reply::Exception(1));
    let frames = control.frames();
    assert_eq!(frames[frames.len() - 2].kind, FrameKind::Write);
    assert_eq!(frames[frames.len() - 1].kind, FrameKind::Other);
    // Another unit identifier: the answer a gateway gives. 0xFF is this device.
    client.unit = 9;
    assert_eq!(
        client.ask(&read_pdu(0x03, SETPOINT, 1)).await?,
        Reply::Exception(0x0B)
    );
    client.unit = 0xFF;
    assert_eq!(client.holding(SETPOINT, 1).await?, [0]);

    assert!(
        control
            .frames()
            .iter()
            .take(frames.len() + 1)
            .all(|f| !f.applied)
    );
    assert!(control.inspect().write_counts.is_empty());

    // A header that is not this protocol's: no reply, and the link closes.
    client.stream.write_all(&[0, 1, 0, 9, 0, 6, 1]).await?;
    assert!(client.closed().await?);
    let last = control.frames().pop().ok_or("no frame")?;
    assert_eq!(
        (last.request, last.kind, last.answer),
        (Request::BadHeader, FrameKind::Other, Answer::None)
    );
    Ok(())
}

#[tokio::test]
async fn points_change_on_their_own_one_tick_for_every_frame() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    let mut client = Client::connect(&device).await?;

    // Two reads of the same point, two values.
    assert_eq!(client.input(32, 2).await?, [0, 0]);
    assert_eq!(client.input(32, 2).await?, [0, 1]);

    // The pump follows its setpoint by one ramp step per tick, and the valve
    // follows its command at the next tick: the world lags what the device holds.
    control.set_process_frozen(true);
    client.write(SETPOINT, 10_000).await?;
    client.data(&write_coil_pdu(VALVE, true)).await?;
    assert_eq!(client.holding(SETPOINT, 1).await?, [10_000]);
    assert_eq!(client.input(30, 1).await?, [0]);
    assert_eq!(client.bits(0x02, 0, 2).await?, [false, false]);
    assert_eq!(
        (control.outputs().pump_speed, control.outputs().valve_open),
        (0, false)
    );

    control.advance(1);
    assert_eq!(
        (control.outputs().pump_speed, control.outputs().valve_open),
        (4000, true)
    );
    assert_eq!(client.bits(0x02, 0, 2).await?, [true, true]);
    control.advance(5);
    assert_eq!(client.input(30, 1).await?, [10_000]);

    // Unfrozen, the frames themselves move time.
    control.set_process_frozen(false);
    client.write(SETPOINT, 0).await?;
    assert_eq!(client.input(30, 1).await?, [6000]);
    assert_eq!(client.input(30, 1).await?, [2000]);
    assert_eq!(client.input(30, 1).await?, [0]);
    Ok(())
}

#[tokio::test]
async fn a_link_dropped_after_a_write_was_received_is_a_possible_commit() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    control.set_fault(Fault::once(FaultKind::DropLink { applied: true }).on(Scope::Writes));
    let mut client = Client::connect(&device).await?;

    // The read is untouched: the fault waits for a write.
    assert_eq!(client.holding(SETPOINT, 1).await?, [0]);
    assert!(
        client
            .ask(&write_register_pdu(SETPOINT, 7000))
            .await
            .is_err()
    );
    assert!(client.closed().await?);

    // The client saw no answer, and the device holds the value.
    assert_eq!(number(&device, "pump_speed_setpoint")?, 7000);
    assert_eq!(control.write_count("pump_speed_setpoint"), 1);
    let frame = control.frames().pop().ok_or("no frame")?;
    assert_eq!((frame.applied, frame.answer), (true, Answer::None));
    assert_eq!(frame.fault, Some(FaultKind::DropLink { applied: true }));
    assert!(control.faults().is_empty());

    // The other half: the link drops and nothing was stored.
    control.set_fault(Fault::once(FaultKind::DropLink { applied: false }).on(Scope::Writes));
    let mut client = Client::connect(&device).await?;
    assert!(
        client
            .ask(&write_register_pdu(SETPOINT, 8000))
            .await
            .is_err()
    );
    assert_eq!(number(&device, "pump_speed_setpoint")?, 7000);
    let mut client = Client::connect(&device).await?;
    assert_eq!(client.holding(SETPOINT, 1).await?, [7000]);
    assert_eq!(control.inspect().open_links, 1);
    Ok(())
}

#[tokio::test]
async fn a_frame_with_no_reply_leaves_the_link_open() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    control.set_fault(Fault::once(FaultKind::NoReply { applied: true }).on(Scope::Writes));
    let mut client = Client::connect(&device).await?;

    client.send(&write_register_pdu(SETPOINT, 3000)).await?;
    // Wait for the device to have the frame, not for a time to pass.
    assert!(control.wait_until(PATIENCE, |now| now.frames == 1).await);
    assert_eq!(number(&device, "pump_speed_setpoint")?, 3000);

    // The next request on the same link is answered, and the answer is its
    // own: nothing was ever sent for the first.
    let second = client.send(&read_pdu(0x03, SETPOINT, 1)).await?;
    let (transaction, reply) = client.receive().await?;
    assert_eq!((transaction, reply), (second, vec![0x03, 2, 0x0B, 0xB8]));
    let first = &control.frames()[0];
    assert_eq!((first.applied, first.answer), (true, Answer::None));
    assert_eq!(first.fault, Some(FaultKind::NoReply { applied: true }));
    Ok(())
}

#[tokio::test]
async fn a_slow_reply_an_exception_and_a_busy_device_can_each_be_switched_on() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    let mut client = Client::connect(&device).await?;

    control.set_fault(Fault::once(FaultKind::Delay { ms: 60 }));
    let started = Instant::now();
    client.write(SETPOINT, 500).await?;
    assert!(started.elapsed() >= Duration::from_millis(60));
    let slow = control.frames().pop().ok_or("no frame")?;
    assert_eq!((slow.applied, slow.answer), (true, Answer::Normal));
    assert_eq!(slow.fault, Some(FaultKind::Delay { ms: 60 }));

    // A forced exception stores nothing.
    control.set_fault(Fault::once(FaultKind::Exception { code: 4 }));
    assert_eq!(
        client.ask(&write_register_pdu(SETPOINT, 900)).await?,
        Reply::Exception(4)
    );
    assert_eq!(number(&device, "pump_speed_setpoint")?, 500);

    // Busy twice, for writes only; reads go through, and then the write does.
    control.set_fault(Fault::busy().on(Scope::Writes).times(2));
    assert_eq!(
        client.ask(&write_register_pdu(SETPOINT, 900)).await?,
        Reply::Exception(6)
    );
    assert_eq!(client.holding(SETPOINT, 1).await?, [500]);
    assert_eq!(
        client.ask(&write_register_pdu(SETPOINT, 900)).await?,
        Reply::Exception(6)
    );
    client.write(SETPOINT, 900).await?;
    assert_eq!(control.write_count("pump_speed_setpoint"), 2);
    assert_eq!(control.inspect().faults, 0);
    Ok(())
}

#[tokio::test]
async fn a_corrupted_reply_reaches_the_client_after_the_write_was_stored() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    let mut client = Client::connect(&device).await?;

    control.set_fault(Fault::once(FaultKind::CorruptReply(
        Corruption::TransactionId,
    )));
    let sent = client.send(&write_register_pdu(SETPOINT, 100)).await?;
    let (transaction, _) = client.receive().await?;
    assert_eq!(transaction, sent + 1);

    control.set_fault(Fault::once(FaultKind::CorruptReply(Corruption::LastByte)));
    let echo = client.data(&write_register_pdu(SETPOINT, 0x0200)).await?;
    assert_eq!(echo, [0, 100, 0x02, 0xFF]);

    control.set_fault(Fault::once(FaultKind::CorruptReply(Corruption::Truncated)));
    assert!(
        client
            .ask(&write_register_pdu(SETPOINT, 300))
            .await
            .is_err()
    );

    // Each write was stored whatever the client made of the reply.
    assert_eq!(number(&device, "pump_speed_setpoint")?, 300);
    assert_eq!(control.write_count("pump_speed_setpoint"), 3);
    let frames = control.frames();
    assert!(
        frames
            .iter()
            .all(|f| f.applied && f.answer == Answer::Normal)
    );
    assert_eq!(
        frames[2].fault,
        Some(FaultKind::CorruptReply(Corruption::Truncated))
    );
    Ok(())
}

#[tokio::test]
async fn a_restart_resets_volatile_points_and_keeps_the_rest() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    let mut client = Client::connect(&device).await?;
    client.write(SETPOINT, 4000).await?;
    client.write(DOSING_RATE, 700).await?;
    client.data(&write_coil_pdu(VALVE, true)).await?;
    assert!(control.outputs().valve_open);

    // The link that was open is gone: a frame sent on it is never handled.
    control.restart();
    let received = control.frames().len();
    assert!(
        client
            .ask(&write_register_pdu(SETPOINT, 4000))
            .await
            .is_err()
    );
    assert!(client.closed().await?);
    assert_eq!(control.frames().len(), received);

    let now = control.inspect();
    assert_eq!((now.restarts, now.powered), (1, true));
    assert_eq!(now.points["pump_speed_setpoint"], Value::Number(0));
    assert_eq!(now.points["inlet_valve"], Value::Bit(false));
    assert_eq!(now.points["uptime_ticks"], Value::Number(0));
    assert_eq!(now.points["dosing_rate"], Value::Number(700));
    assert!(!now.outputs.valve_open);

    // A restart in the middle of a run: the second write from now is lost to it.
    control.set_fault(Fault::once(FaultKind::Restart).on(Scope::Writes).after(1));
    let mut client = Client::connect(&device).await?;
    client.write(SETPOINT, 1000).await?;
    assert_eq!(client.holding(SETPOINT, 1).await?, [1000]);
    assert!(
        client
            .ask(&write_register_pdu(DOSING_RATE, 900))
            .await
            .is_err()
    );
    assert_eq!(number(&device, "pump_speed_setpoint")?, 0);
    assert_eq!(number(&device, "dosing_rate")?, 700);
    let lost = control.frames().pop().ok_or("no frame")?;
    assert_eq!(
        (lost.applied, lost.answer, lost.fault),
        (false, Answer::None, Some(FaultKind::Restart))
    );
    assert_eq!(control.inspect().restarts, 2);
    Ok(())
}

#[tokio::test]
async fn the_device_reverts_to_its_safe_state_on_link_loss_when_told_to() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    let mut client = Client::connect(&device).await?;
    client.write(SETPOINT, 5000).await?;
    client.data(&write_coil_pdu(VALVE, true)).await?;

    // Off at the start: a closed link changes nothing.
    drop(client);
    assert!(
        control
            .wait_until(PATIENCE, |now| now.open_links == 0)
            .await
    );
    assert_eq!(number(&device, "pump_speed_setpoint")?, 5000);

    control.set_revert_on_link_loss(true);
    let client = Client::connect(&device).await?;
    assert!(
        control
            .wait_until(PATIENCE, |now| now.open_links == 1)
            .await
    );
    drop(client);
    assert!(control.wait_until(PATIENCE, |now| now.reverts == 1).await);
    assert_eq!(number(&device, "pump_speed_setpoint")?, 0);
    assert_eq!(control.bit("inlet_valve"), Some(false));

    // With the dropped-link fault, the write lands and is reverted before the
    // client can come back: what it reads is the safe state, not what it sent.
    control.set_fault(Fault::once(FaultKind::DropLink { applied: true }).on(Scope::Writes));
    let mut client = Client::connect(&device).await?;
    assert!(
        client
            .ask(&write_register_pdu(SETPOINT, 6000))
            .await
            .is_err()
    );
    assert!(client.closed().await?);
    let mut client = Client::connect(&device).await?;
    assert_eq!(client.holding(SETPOINT, 1).await?, [0]);
    assert_eq!(control.write_count("pump_speed_setpoint"), 2);
    assert_eq!(control.inspect().reverts, 2);
    Ok(())
}

#[tokio::test]
async fn a_device_that_is_down_closes_every_link_until_the_power_returns() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    let mut client = Client::connect(&device).await?;
    client.write(SETPOINT, 2000).await?;

    control.power_off();
    assert!(client.closed().await?);
    let mut client = Client::connect(&device).await?;
    assert!(client.ask(&read_pdu(0x03, SETPOINT, 1)).await.is_err());
    assert_eq!(control.frames().len(), 1);
    assert!(!control.inspect().powered);

    control.power_on();
    let mut client = Client::connect(&device).await?;
    assert_eq!(client.holding(SETPOINT, 1).await?, [0]);
    Ok(())
}

/// One block of an image, as the block window takes it.
fn block(number: u16, bytes: &[u8]) -> Vec<u16> {
    let mut words = vec![number, bytes.len() as u16, firmware::crc16(bytes)];
    words.extend(
        bytes
            .chunks(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair.get(1).copied().unwrap_or(0)])),
    );
    words
}

/// Declares an image, enters update mode, and sends every block.
async fn send_image(client: &mut Client, image: &[u8]) -> Outcome {
    client
        .write(UPDATE_IMAGE_LENGTH, image.len() as u16)
        .await?;
    client.write(UPDATE_COMMAND, 1).await?;
    for (number, bytes) in image.chunks(MAX_BLOCK_BYTES).enumerate() {
        client
            .data(&write_registers_pdu(
                UPDATE_BLOCK,
                &block(number as u16, bytes),
            ))
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn the_update_sequence_installs_an_image_and_can_be_rolled_back() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    let before = control.firmware();
    assert_eq!(
        (before.version, before.active_slot, before.update_count),
        (Version::new(2, 4, 0), 0, 1)
    );
    let image = firmware::build_image(MODEL_CODE, Version::new(2, 5, 0), &[7u8; 500]);
    let checksum = firmware::image_checksum(&image).ok_or("no checksum")?;

    // Enter, send the blocks, verify.
    let mut client = Client::connect(&device).await?;
    send_image(&mut client, &image).await?;
    assert_eq!(client.input(130, 5).await?, [1, 0, 3, 518, 1]);
    assert_eq!(client.input(121, 1).await?, [0]);
    client.write(UPDATE_COMMAND, 2).await?;
    assert_eq!(client.input(130, 1).await?, [2]);
    assert_eq!(control.firmware().update_state, UpdateState::Verified);
    assert_eq!(control.firmware().version, Version::new(2, 4, 0));

    // Commit: the device answers, restarts, and comes back on the new image.
    client.write(UPDATE_COMMAND, 3).await?;
    assert!(client.closed().await?);
    let mut client = Client::connect(&device).await?;
    let words = client.input(100, 25).await?;
    assert_eq!(words[..3], [2, 5, 0]);
    assert_eq!(
        (u32::from(words[18]) << 16) | u32::from(words[19]),
        checksum
    );
    assert_eq!(words[20..], [1, 1, 2, 4, 0]);
    assert_eq!(client.input(130, 5).await?, [0, 0, 0, 0, 2]);
    let after = control.firmware();
    assert_eq!(
        (after.version, after.checksum, after.active_slot),
        (Version::new(2, 5, 0), checksum, 1)
    );
    assert_eq!((after.update_count, control.inspect().restarts), (2, 1));
    assert_eq!(control.write_count("update_block"), 3);
    assert_eq!(control.write_count("update_command"), 3);

    // Roll back: the earlier image runs again, and the counter does not go down.
    client.write(UPDATE_COMMAND, 5).await?;
    assert!(client.closed().await?);
    let mut client = Client::connect(&device).await?;
    assert_eq!(client.input(100, 3).await?, [2, 4, 0]);
    assert_eq!(client.input(134, 1).await?, [2]);
    assert_eq!(control.firmware().active_slot, 0);
    Ok(())
}

#[tokio::test]
async fn the_update_mechanism_refuses_what_it_should_and_says_why() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    control.set_process_frozen(true);
    let mut client = Client::connect(&device).await?;
    let image = firmware::build_image(MODEL_CODE, Version::new(2, 5, 0), &[1u8; 300]);
    let enter = write_register_pdu(UPDATE_COMMAND, 1);
    let state = |control: &example_device::Control| {
        let firmware = control.firmware();
        (firmware.update_state, firmware.update_error)
    };

    // Not without a declared length, not with the pump running, not an unknown command.
    assert_eq!(client.ask(&enter).await?, Reply::Exception(4));
    assert_eq!(
        state(control),
        (UpdateState::Idle, UpdateError::LengthNotDeclared)
    );
    client
        .write(UPDATE_IMAGE_LENGTH, image.len() as u16)
        .await?;
    client.write(SETPOINT, 100).await?;
    assert_eq!(client.ask(&enter).await?, Reply::Exception(4));
    assert_eq!(
        client.input(131, 1).await?,
        [UpdateError::PumpRunning.code()]
    );
    client.write(SETPOINT, 0).await?;
    assert_eq!(
        client.ask(&write_register_pdu(UPDATE_COMMAND, 9)).await?,
        Reply::Exception(3)
    );
    assert_eq!(
        client.ask(&write_register_pdu(UPDATE_COMMAND, 3)).await?,
        Reply::Exception(4)
    );
    assert_eq!(state(control).1, UpdateError::WrongState);
    let both = write_registers_pdu(UPDATE_COMMAND, &[1, 300]);
    assert_eq!(client.ask(&both).await?, Reply::Exception(2));
    assert!(control.firmware().slots[1].valid);

    // In update mode the standby slot is given up, and the process takes no writes.
    client.write(UPDATE_COMMAND, 1).await?;
    assert!(!control.firmware().slots[1].valid);
    assert_eq!(
        client.ask(&write_register_pdu(SETPOINT, 100)).await?,
        Reply::Exception(6)
    );
    assert_eq!(
        client.ask(&write_coil_pdu(VALVE, true)).await?,
        Reply::Exception(6)
    );
    assert_eq!(client.holding(SETPOINT, 1).await?, [0]);

    // Blocks: the wrong number, a bad checksum, a wrong byte count, too many bytes.
    let first = &image[..MAX_BLOCK_BYTES];
    let ask_block = |words: Vec<u16>| write_registers_pdu(UPDATE_BLOCK, &words);
    assert_eq!(
        client.ask(&ask_block(block(1, first))).await?,
        Reply::Exception(4)
    );
    assert_eq!(state(control).1, UpdateError::BlockOutOfOrder);
    let mut damaged = block(0, first);
    damaged[5] ^= 1;
    assert_eq!(client.ask(&ask_block(damaged)).await?, Reply::Exception(3));
    assert_eq!(state(control).1, UpdateError::BlockChecksum);
    let mut miscounted = block(0, first);
    miscounted[1] = 10;
    assert_eq!(
        client.ask(&ask_block(miscounted)).await?,
        Reply::Exception(3)
    );
    assert_eq!(state(control).1, UpdateError::BlockLength);
    assert_eq!(
        client.ask(&write_register_pdu(UPDATE_BLOCK, 0)).await?,
        Reply::Exception(3)
    );
    assert_eq!(
        client.ask(&write_register_pdu(UPDATE_BLOCK + 1, 0)).await?,
        Reply::Exception(2)
    );
    assert_eq!(control.firmware().bytes_received, 0);
    client.data(&ask_block(block(0, first))).await?;
    // The same block again is refused: a block is never taken twice.
    assert_eq!(
        client.ask(&ask_block(block(0, first))).await?,
        Reply::Exception(4)
    );
    assert_eq!(control.firmware().bytes_received, MAX_BLOCK_BYTES);

    // Verifying too early fails the image; only an abort leaves that state.
    client.write(UPDATE_COMMAND, 2).await?;
    assert_eq!(
        state(control),
        (UpdateState::Failed, UpdateError::ImageIncomplete)
    );
    assert_eq!(
        client.ask(&write_register_pdu(UPDATE_COMMAND, 3)).await?,
        Reply::Exception(4)
    );
    client.write(UPDATE_COMMAND, 4).await?;
    assert_eq!(state(control), (UpdateState::Idle, UpdateError::None));
    assert_eq!(client.input(7, 1).await?, [2]);

    // After an abort there is nothing to roll back to.
    assert_eq!(
        client.ask(&write_register_pdu(UPDATE_COMMAND, 5)).await?,
        Reply::Exception(4)
    );
    assert_eq!(state(control).1, UpdateError::NoStandbyImage);

    // An image for another model, a damaged image, and a good image the device refuses.
    let other = firmware::build_image(0x0001, Version::new(2, 5, 0), &[1u8; 300]);
    let mut flipped = image.clone();
    flipped[20] ^= 0x10;
    for (bytes, error) in [
        (&other, UpdateError::ImageWrongModel),
        (&flipped, UpdateError::ImageChecksum),
        (&image, UpdateError::ImageRefused),
    ] {
        control.set_refuse_image(error == UpdateError::ImageRefused);
        send_image(&mut client, bytes).await?;
        client.write(UPDATE_COMMAND, 2).await?;
        assert_eq!(state(control), (UpdateState::Failed, error));
        client.write(UPDATE_COMMAND, 4).await?;
    }
    assert_eq!(control.firmware().version, Version::new(2, 4, 0));
    assert_eq!(control.firmware().update_count, 1);
    assert_eq!(control.inspect().restarts, 0);
    Ok(())
}

#[tokio::test]
async fn a_device_that_does_not_come_back_is_reached_by_its_recovery_route() -> Outcome {
    let device = TestDevice::start().await?;
    let control = device.control();
    control.set_stay_down_after_restart(true);
    let image = firmware::build_image(
        MODEL_CODE,
        Version::new(2, 5, 0),
        b"an image that will not start",
    );
    let mut client = Client::connect(&device).await?;
    send_image(&mut client, &image).await?;
    client.write(UPDATE_COMMAND, 2).await?;

    // The commit is answered, and then the device is gone.
    client.write(UPDATE_COMMAND, 3).await?;
    assert!(client.closed().await?);
    let mut client = Client::connect(&device).await?;
    assert!(client.ask(&read_pdu(0x04, 100, 3)).await.is_err());
    let down = control.inspect();
    assert_eq!(
        (down.powered, down.firmware.version),
        (false, Version::new(2, 5, 0))
    );
    assert_eq!(down.firmware.update_count, 2);

    // The recovery route starts the device from the slot it ran before.
    assert!(control.recover());
    let mut client = Client::connect(&device).await?;
    assert_eq!(client.input(100, 3).await?, [2, 4, 0]);
    assert_eq!(client.input(134, 1).await?, [2]);

    // With no image in the standby slot the route has nothing to start from but the same slot.
    let config = DeviceConfig {
        standby_firmware: None,
        ..DeviceConfig::default()
    };
    let single = TestDevice::start_with(config).await?;
    single.control().power_off();
    assert!(!single.control().recover());
    assert_eq!(single.control().firmware().version, Version::new(2, 4, 0));
    assert!(single.control().inspect().powered);
    Ok(())
}

#[tokio::test]
async fn another_firmware_version_can_be_reported() -> Outcome {
    let device = TestDevice::start().await?;
    device.control().set_firmware_version(Version::new(3, 0, 1));
    let mut client = Client::connect(&device).await?;
    assert_eq!(client.input(100, 3).await?, [3, 0, 1]);
    let one = client.data(&[0x2B, 0x0E, 4, 2]).await?;
    assert!(one.ends_with(b"3.0.1"));
    Ok(())
}

#[tokio::test]
async fn reset_restores_the_starting_state_and_two_devices_share_nothing() -> Outcome {
    let first = TestDevice::start().await?;
    let second = TestDevice::start_with(DeviceConfig {
        unit: 7,
        ..DeviceConfig::default()
    })
    .await?;
    assert_ne!(first.port(), second.port());
    assert_eq!((first.unit(), second.unit()), (1, 7));
    let start = first.control().inspect();

    let mut client = Client::connect(&first).await?;
    client.write(SETPOINT, 1234).await?;
    client.write(DOSING_RATE, 800).await?;
    first
        .control()
        .set_fault(Fault::always(FaultKind::Exception { code: 4 }));
    first.control().set_refuse_image(true);
    first.control().restart();
    assert_eq!(number(&second, "dosing_rate")?, 250);
    assert!(second.control().frames().is_empty());
    let mut other = Client::connect(&second).await?;
    assert_eq!(other.holding(DOSING_RATE, 1).await?, [250]);

    assert!(
        first
            .control()
            .wait_until(PATIENCE, |now| now.open_links == 0)
            .await
    );
    first.control().reset();
    assert_eq!(first.control().inspect(), start);
    let mut client = Client::connect(&first).await?;
    assert_eq!(client.holding(DOSING_RATE, 1).await?, [250]);
    Ok(())
}
