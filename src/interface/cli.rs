use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "nandpromax")]
#[command(about = "Unified NAND / eMMC Flasher Tool (PicoFlasher, FTDI, LPC, DemoN)", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Picoflasher,
    Ftdi,
    Lpc,
    Demon,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterType {
    Usb,
    Tcp,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Spi,
    Emmc,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtdiPageFormat {
    Auto,
    Small,
    Big,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read NAND / eMMC to a file
    ReadNand {
        #[arg(long, help = "Output file path")]
        out: PathBuf,

        #[arg(long, value_enum, help = "Hardware device (Priority: picoflasher -> ftdi -> lpc -> demon)")]
        device: Option<DeviceType>,

        #[arg(long, value_enum, help = "Interface adapter (Priority: usb -> tcp)")]
        adapter: Option<AdapterType>,

        #[arg(long = "type", value_enum, help = "Media type (Priority: spi -> emmc)")]
        media_type: Option<MediaType>,

        #[arg(long, default_value_t = 0, help = "Start block / LBA offset")]
        start: u32,

        #[arg(long, help = "Number of blocks / LBAs to read")]
        count: Option<u32>,

        #[arg(long, help = "USB Serial port (e.g. /dev/ttyACM0)")]
        serial: Option<String>,

        #[arg(long, alias = "ip", default_value = "192.168.4.1:3232", help = "TCP IP address:port")]
        addr: String,

        #[arg(long, default_value = "auto", help = "FTDI device description filter")]
        ftdi_desc: String,

        #[arg(long, help = "FTDI device index")]
        ftdi_index: Option<i32>,

        #[arg(long, default_value_t = 6_000_000, help = "SPI frequency in Hz")]
        freq_hz: u32,

        #[arg(long, value_enum, default_value_t = FtdiPageFormat::Auto, help = "FTDI page format")]
        page_format: FtdiPageFormat,

        #[arg(long, default_value_t = 3000, help = "Operation timeout in milliseconds")]
        timeout_ms: u64,
    },

    /// Write file to NAND / eMMC
    WriteNand {
        #[arg(long, help = "Input file path")]
        input: PathBuf,

        #[arg(long, value_enum, help = "Hardware device (Priority: picoflasher -> ftdi -> lpc -> demon)")]
        device: Option<DeviceType>,

        #[arg(long, value_enum, help = "Interface adapter (Priority: usb -> tcp)")]
        adapter: Option<AdapterType>,

        #[arg(long = "type", value_enum, help = "Media type (Priority: spi -> emmc)")]
        media_type: Option<MediaType>,

        #[arg(long, default_value_t = 0, help = "Start block / LBA offset")]
        start: u32,

        #[arg(long, help = "Number of blocks to write")]
        count: Option<u32>,

        #[arg(long, action = ArgAction::Set, default_value_t = true, help = "Erase block before writing")]
        erase: bool,

        #[arg(long, help = "Verify block after writing")]
        verify: bool,

        #[arg(long, help = "USB Serial port (e.g. /dev/ttyACM0)")]
        serial: Option<String>,

        #[arg(long, alias = "ip", default_value = "192.168.4.1:3232", help = "TCP IP address:port")]
        addr: String,

        #[arg(long, default_value = "auto", help = "FTDI device description filter")]
        ftdi_desc: String,

        #[arg(long, help = "FTDI device index")]
        ftdi_index: Option<i32>,

        #[arg(long, default_value_t = 6_000_000, help = "SPI frequency in Hz")]
        freq_hz: u32,

        #[arg(long, value_enum, default_value_t = FtdiPageFormat::Auto, help = "FTDI page format")]
        page_format: FtdiPageFormat,

        #[arg(long, default_value_t = 3000, help = "Operation timeout in milliseconds")]
        timeout_ms: u64,
    },

    /// Display device and flash memory information
    Info {
        #[arg(long, value_enum, help = "Hardware device (Priority: picoflasher -> ftdi -> lpc -> demon)")]
        device: Option<DeviceType>,

        #[arg(long, value_enum, help = "Interface adapter (Priority: usb -> tcp)")]
        adapter: Option<AdapterType>,

        #[arg(long, help = "USB Serial port")]
        serial: Option<String>,

        #[arg(long, alias = "ip", default_value = "192.168.4.1:3232", help = "TCP IP address:port")]
        addr: String,

        #[arg(long, default_value = "auto")]
        ftdi_desc: String,

        #[arg(long)]
        ftdi_index: Option<i32>,

        #[arg(long, default_value_t = 6_000_000)]
        freq_hz: u32,

        #[arg(long, default_value_t = 3000)]
        timeout_ms: u64,
    },

    /// List connected flasher hardware devices
    ListDevices,

    /// Stream POST codes from FTDI or serial port
    ReadPost {
        #[arg(long, help = "Output file path")]
        out: Option<PathBuf>,

        #[arg(long, help = "Maximum POST byte count")]
        count: Option<u64>,

        #[arg(long, value_enum)]
        device: Option<DeviceType>,

        #[arg(long)]
        serial: Option<String>,

        #[arg(long, default_value_t = 115200)]
        baud: u32,

        #[arg(long)]
        quiet: bool,

        #[arg(long, default_value = "auto")]
        ftdi_desc: String,

        #[arg(long)]
        ftdi_index: Option<i32>,

        #[arg(long, default_value_t = 1000)]
        poll_us: u64,
    },

    /// Run as a TCP device server bridging hardware flasher over network
    ServeTcp {
        #[arg(long, default_value = "0.0.0.0:8383", help = "Bind address:port")]
        bind: String,

        #[arg(long, value_enum, help = "Hardware device to serve over TCP")]
        device: Option<DeviceType>,
    },
}
