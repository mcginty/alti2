use anyhow::{ensure, Error};
use serial::{
    core::{SerialDevice, SerialPortSettings},
    BaudRate, CharSize, FlowControl, Parity, StopBits,
};
use std::{
    convert::TryFrom,
    fmt,
    io::{Read, Write},
    path::Path,
    thread::sleep,
    time::Duration,
};

static TTY_TIMEOUT: Duration = Duration::from_millis(10000);
static PAUSE_BEFORE_HANDSHAKE: Duration = Duration::from_secs(10);

fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |acc, b| acc.wrapping_add(*b))
}

#[derive(Debug)]
struct SoftwareVersion {
    major: usize,
    minor: usize,
    revision: usize,
}

impl fmt::Display for SoftwareVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.revision)
    }
}

#[derive(Debug)]
enum ProductType {
    Neptune,
    Wave,
    Tracker,
    DataLogger,
    N3,
    N3A,
    Atlas,
    Unknown,
}

impl From<u8> for ProductType {
    fn from(code: u8) -> Self {
        match code {
            1 => ProductType::Neptune,
            2 => ProductType::Wave,
            3 => ProductType::Tracker,
            4 => ProductType::DataLogger,
            5 => ProductType::N3,
            6 => ProductType::N3A,
            7 => ProductType::Atlas,
            _ => ProductType::Unknown,
        }
    }
}

#[derive(Debug)]
struct DeviceInfo {
    original_record: Vec<u8>,
    comm_type: u8,
    sw_version: SoftwareVersion,
    serial_number: String,
    hardware_revision: u8,
    product_type: ProductType,
    nvram_config: u8,
}

impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Alti-2 {:?} (rev. {}, S/N {}, S/W {})",
            self.product_type, self.hardware_revision, self.serial_number, self.sw_version
        )
    }
}

impl<'a> TryFrom<&'a [u8]> for DeviceInfo {
    type Error = Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        ensure!(checksum(&bytes[1..bytes.len()-1]) == bytes[bytes.len()-1], "checksum mismatch");

        Ok(Self {
            original_record: bytes.to_vec(),
            comm_type: bytes[2],
            sw_version: SoftwareVersion {
                major: (bytes[3] >> 4) as usize,
                minor: (bytes[3] & 0x0f) as usize,
                revision: bytes[4] as usize,
            },
            serial_number: String::from_utf8(bytes[5..14].to_vec())?.trim().to_string(),
            hardware_revision: bytes[14],
            product_type: ProductType::from(bytes[15]),
            nvram_config: bytes[16],
        })
    }
}

enum Command {
    GetInfo,
}

impl Command {
    /// Handles all the weird formatting the device expects. Ex: turns the 0x80 command into
    /// the string "018080", prepending the length and appending the checksum.
    pub fn to_bytes(&self) -> Vec<u8> {
        let contents = match self {
            Command::GetInfo => vec![0x80],
        };
        let mut bytes = vec![];
        bytes.extend_from_slice(hex::encode_upper(&[contents.len() as u8]).as_bytes());
        bytes.extend_from_slice(hex::encode_upper(&contents).as_bytes());
        bytes.extend_from_slice(hex::encode_upper(&[checksum(&contents)]).as_bytes());
        bytes
    }
}

struct AltiTTY {
    tty: serial::unix::TTYPort,
}

impl AltiTTY {
    fn open(path: &str) -> Result<Self, Error> {
        let mut tty = serial::unix::TTYPort::open(Path::new(path)).unwrap();
        tty.set_timeout(TTY_TIMEOUT).unwrap();
        let mut settings = tty.read_settings().unwrap();
        settings.set_baud_rate(BaudRate::Baud57600).unwrap();
        settings.set_char_size(CharSize::Bits8);
        settings.set_stop_bits(StopBits::Stop1);
        settings.set_parity(Parity::ParityNone);
        settings.set_flow_control(FlowControl::FlowHardware);
        tty.write_settings(&settings).unwrap();
        tty.set_dtr(true).unwrap();

        sleep(PAUSE_BEFORE_HANDSHAKE);
        Ok(Self { tty })
    }

    fn device_info(&mut self) -> Result<DeviceInfo, Error> {
        self.tty.write_all(&Command::GetInfo.to_bytes())?;

        sleep(Duration::from_millis(100));

        let mut buf = [0u8; 1024];

        // Read just the length (in "XX " hex ASCII format).
        self.tty.read_exact(&mut buf[..3])?;
        let len = hex::decode(String::from_utf8(buf[0..2].to_vec())?)?[0] as usize;

        // (len+1) to include the checksum byte, multiply by 3 for each "XX " spaced combination of hex,
        // then add 2 for the "\r\n" ending.
        let remaining_ascii_len = (len + 1) * 3 + 2;
        self.tty.read_exact(&mut buf[3..3 + remaining_ascii_len])?;

        let info_str = String::from_utf8(buf[..2 + remaining_ascii_len].to_vec())?;
        let stripped_str = info_str.replace(&[' ', '\n', '\r'][..], "");
        let info_bytes = hex::decode(&stripped_str)?;
        let device_info = DeviceInfo::try_from(&info_bytes[..])?;

        Ok(device_info)
    }
}

fn main() -> Result<(), Error> {
    println!("opening TTY (will pause for 10 seconds to wait for ready state)");
    let mut tty = AltiTTY::open("/dev/ttyUSB0")?;
    println!("successfully opened TTY.");

    println!("requesting device info.");
    let device_info = tty.device_info()?;
    println!("device: {}", device_info);

    Ok(())
}
