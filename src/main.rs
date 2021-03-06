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
    num::Wrapping,
    thread::sleep,
    time::Duration,
};

static TTY_TIMEOUT: Duration = Duration::from_millis(10000);
static PAUSE_BEFORE_HANDSHAKE: Duration = Duration::from_secs(10);

fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |acc, b| acc.wrapping_add(*b))
}

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

struct DeviceInfo {
    sw_version: SoftwareVersion,
    serial_number: String,
    hardware_revision: u8,
    product_type: ProductType,
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
        ensure!(
            checksum(&bytes[1..bytes.len() - 1]) == bytes[bytes.len() - 1],
            "checksum mismatch"
        );

        Ok(Self {
            sw_version: SoftwareVersion {
                major: (bytes[3] >> 4) as usize,
                minor: (bytes[3] & 0x0f) as usize,
                revision: bytes[4] as usize,
            },
            serial_number: String::from_utf8(bytes[5..14].to_vec())?.trim().to_string(),
            hardware_revision: bytes[14],
            product_type: ProductType::from(bytes[15]),
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

struct Cipher {
    k: [u32; 4],
}

impl Cipher {
    fn from_type0_bytes(bytes: &[u8]) -> Self {
        Self {
            k: [
                u32::from_le_bytes([78, bytes[8], bytes[26], bytes[24]]),
                u32::from_le_bytes([bytes[6], bytes[25], bytes[23], bytes[13]]),
                u32::from_le_bytes([bytes[10], 117, bytes[7], bytes[22]]),
                u32::from_le_bytes([bytes[9], bytes[11], 126, bytes[21]]),
            ],
        }
    }

    pub fn encrypt_single(&self, v: &[u32]) -> [u32; 2] {
        let mut u = Wrapping(v[0]);
        let mut u1 = Wrapping(v[1]);
        let mut u2 = Wrapping(0u32);

        for _ in 0..16 {
            u += (((u1 << 4) ^ (u1 >> 5)) + u1) ^ (u2 + Wrapping(self.k[(u2.0 & 3) as usize]));
            u2 += Wrapping(0x9E3779B9);
            u1 += (((u << 4) ^ (u >> 5)) + u) ^ (u2 + Wrapping(self.k[((u2.0 >> 11) & 3) as usize]));
        }

        [u.0, u1.0]
    }

    pub fn decrypt_single(&self, v: &[u32]) -> [u32; 2] {
        let mut u = Wrapping(v[0]);
        let mut u1 = Wrapping(v[1]);
        let mut u2 = Wrapping(0xE3779B90);

        for _ in 0..16 {
            u1 -= (((u << 4) ^ (u >> 5)) + u) ^ (u2 + Wrapping(self.k[((u2.0 >> 11) & 3) as usize]));
            u2 -= Wrapping(0x9E3779B9);
            u -= (((u1 << 4) ^ (u1 >> 5)) + u1) ^ (u2 + Wrapping(self.k[(u2.0 & 3) as usize]));
        }

        [u.0, u1.0]
    }

    pub fn encrypt(&self, bytes: &[u8]) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        let len = bytes.len();
        bytes.resize(len + (if len % 32 != 0 { 32 - len % 32 } else { 0 }), 0);

        let u32s: Vec<u32> = bytes
            .chunks(4)
            .map(|chunk| {
                let mut b = [0u8; 4];
                b[..chunk.len()].copy_from_slice(chunk);
                u32::from_le_bytes(b)
            })
            .collect();

        let pairs = u32s.chunks_exact(2);
        pairs
            .map(|pair| {
                let enc_pair = self.encrypt_single(pair);
                let mut bytes = Vec::with_capacity(8);
                bytes.extend_from_slice(&enc_pair[0].to_le_bytes());
                bytes.extend_from_slice(&enc_pair[1].to_le_bytes());
                bytes
            })
            .flatten()
            .collect()
    }

    pub fn decrypt(&self, bytes: &[u8]) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        let len = bytes.len();
        bytes.resize(len + (if len % 32 != 0 { 32 - len % 32 } else { 0 }), 0);

        let u32s: Vec<u32> = bytes
            .chunks(4)
            .map(|chunk| {
                let mut b = [0u8; 4];
                b[..chunk.len()].copy_from_slice(chunk);
                u32::from_le_bytes(b)
            })
            .collect();

        let pairs = u32s.chunks_exact(2);
        pairs
            .map(|pair| {
                let enc_pair = self.decrypt_single(pair);
                let mut bytes = Vec::with_capacity(8);
                bytes.extend_from_slice(&enc_pair[0].to_le_bytes());
                bytes.extend_from_slice(&enc_pair[1].to_le_bytes());
                bytes
            })
            .flatten()
            .collect()
    }
}

struct Session {
    tty: serial::unix::TTYPort,
    pub device_info: DeviceInfo,
    cipher: Cipher,
}

impl Session {
    pub fn open(path: &str) -> Result<Self, Error> {
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
        let type0_bytes = Self::get_type0(&mut tty)?;
        let device_info = DeviceInfo::try_from(&type0_bytes[..])?;
        let cipher = Cipher::from_type0_bytes(&type0_bytes);
        Ok(Self { tty, device_info, cipher })
    }

    fn get_type0(tty: &mut serial::unix::TTYPort) -> Result<Vec<u8>, Error> {
        tty.write_all(&Command::GetInfo.to_bytes())?;

        sleep(Duration::from_millis(100));

        let mut buf = [0u8; 1024];

        // Read just the length (in "XX " hex ASCII format).
        tty.read_exact(&mut buf[..3])?;
        let len = hex::decode(String::from_utf8(buf[0..2].to_vec())?)?[0] as usize;

        // (len+1) to include the checksum byte, multiply by 3 for each "XX " spaced combination of hex,
        // then add 2 for the "\r\n" ending.
        let remaining_ascii_len = (len + 1) * 3 + 2;
        tty.read_exact(&mut buf[3..3 + remaining_ascii_len])?;

        let info_str = String::from_utf8(buf[..2 + remaining_ascii_len].to_vec())?;
        let stripped_str = info_str.replace(&[' ', '\n', '\r'][..], "");
        let info_bytes = hex::decode(&stripped_str)?;

        Ok(info_bytes)
    }
}

fn main() -> Result<(), Error> {
    println!("opening TTY (will pause for 10 seconds to wait for ready state)");
    let mut session = Session::open("/dev/ttyUSB0")?;
    println!("successfully opened TTY.");
    println!("device: {}", session.device_info);

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    static TYPE0_RESPONSE: &[u8] = &[
        0x1E, 0x00, 0x05, 0x10, 0x03, 0x59, 0x31, 0x38, 0x33, 0x36, 0x34, 0x31, 0x20, 0x20, 0x02, 0x07, 0x01, 0x00, 0x20, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x05, 0x00, 0x00, 0x38,
    ];

    // static ENCRYPTED_RESPONSE: &[u8] = &[
    //     0x31, 0x35, 0xe5, 0x73, 0x0e, 0x3f, 0xed, 0xa7, 0x15, 0x52, 0x00, 
    //     0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    //     0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00
    // ];

    #[test]
    fn test_cipher_roundtrip() {
        static TEST_PAYLOAD: &[u8] = &[1, 170, 170];
        let cipher = Cipher::from_type0_bytes(TYPE0_RESPONSE);
        let encrypted = cipher.encrypt(TEST_PAYLOAD);
        assert_eq!(&cipher.decrypt(&encrypted)[..TEST_PAYLOAD.len()], TEST_PAYLOAD);
    }
}
