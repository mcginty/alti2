use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;
use serial::core::{SerialDevice, SerialPortSettings};
use serial::{BaudRate, Parity, CharSize, FlowControl, StopBits};
use std::thread::sleep;
use std::fmt;
use anyhow::Error;

static TTY_TIMEOUT: Duration = Duration::from_millis(10000);
static PAUSE_BEFORE_HANDSHAKE: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct SoftwareVersion {
    major: usize,
    minor: usize,
    revision: usize
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
    Unknown
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

impl<'a> From<&'a [u8]> for DeviceInfo {
    fn from(bytes: &[u8]) -> Self {
        Self {
            original_record: bytes.to_vec(),
            comm_type: bytes[2],
            sw_version: SoftwareVersion {
                major: (bytes[3] >> 4) as usize,
                minor: (bytes[3] & 0x0f) as usize,
                revision: bytes[4] as usize,
            },
            serial_number: String::from_utf8_lossy(&bytes[5..14]).trim().to_string(),
            hardware_revision: bytes[14],
            product_type: ProductType::from(bytes[15]),
            nvram_config: bytes[16],
        }
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
        self.tty.write_all(b"018080")?;

        sleep(Duration::from_millis(100));

        let mut buf = [0u8; 1024];
        let mut pos = 0;

        while let Ok(amount_read) = self.tty.read(&mut buf[pos..]) {
            pos += amount_read;
        }
        let info_str = String::from_utf8(buf[..pos].to_vec())?;
        println!("raw response ({}b): \"{}\"", info_str, info_str.len());

        let stripped_str = info_str.replace(&[' ', '\n', '\r'][..], "");
        println!("stripped: {}", info_str);
        let info_bytes = hex::decode(&stripped_str)?;
        let device_info = DeviceInfo::from(&info_bytes[..]);

        Ok(device_info)
    }
}

fn main() -> Result<(), Error> {

    let mut tty = AltiTTY::open("/dev/ttyUSB0")?;
    println!("successfully opened TTY.");

    println!("requesting device info.");
    let device_info = tty.device_info()?;
    println!("device info: {:?}", device_info);

    Ok(())
}
