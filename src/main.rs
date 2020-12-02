use serialport::{SerialPort, SerialPortSettings, StopBits, posix::TTYPort};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

struct AtlasSession<T: Read + Write + SerialPort> {
    tty: T,
    type0: Vec<u8>,
}

impl<T: Read + Write + SerialPort> AtlasSession<T> {
    fn new(mut tty: T) -> Self {
        let mut buf = [0u8; 256];
        tty.write_all(b"018080").unwrap();
        tty.read_exact(&mut buf[0..1]).unwrap();
        let len = buf[0] as usize;
        println!("got back Type0 of len {}", len);
        tty.read_exact(&mut buf[..len]).unwrap();
        let type0 = buf[..len].to_owned();
        Self { tty, type0 }
    }
}

impl<T: Read + Write + SerialPort> Drop for AtlasSession<T> {
    fn drop(&mut self) {
        self.tty.write_data_terminal_ready(false).unwrap();
    }
}

fn main() {
    let mut tty = TTYPort::open(Path::new("/dev/ttyUSB0"), &SerialPortSettings {
        baud_rate: 57600,
        data_bits: serialport::DataBits::Eight,
        flow_control: serialport::FlowControl::Hardware,
        parity: serialport::Parity::None,
        stop_bits: StopBits::One,
        timeout: Duration::from_secs(20),
    }).unwrap();

    tty.write_data_terminal_ready(true).unwrap();
    std::thread::sleep_ms(1000);
    AtlasSession::new(tty);
}
