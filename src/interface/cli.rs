use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

pub use crate::types::{DeviceType, FtdiPageFormat};

// ---------------------------------------------------------------------------
// Shared argument groups (flattened into subcommands)
// ---------------------------------------------------------------------------

/// Hardware / connection options — shared by all subcommands.
#[derive(Args, Clone, Debug)]
pub struct DeviceArgs {
    /// Hardware device
    #[arg(short = 'd', long, value_enum)]
    pub device: Option<DeviceType>,

    /// Operation timeout in milliseconds
    #[arg(long, default_value_t = 3000)]
    pub timeout_ms: u64,

    /// USB serial port (e.g. /dev/ttyACM0) — PICO/LPC/JRP
    #[arg(long)]
    pub serial: Option<String>,

    /// TCP address:port — ESP / PicoFlasher TCP [default: 192.168.4.1:3232]
    #[arg(long, default_value = "192.168.4.1:3232")]
    pub addr: String,

    /// FTDI device description filter
    #[arg(long, default_value = "auto")]
    pub ftdi_desc: String,

    /// FTDI device index
    #[arg(long)]
    pub ftdi_index: Option<i32>,

    /// SPI clock frequency in Hz
    #[arg(long, default_value_t = 30_000_000)]
    pub freq_hz: u32,

    /// FTDI page format
    #[arg(long, value_enum, default_value_t = FtdiPageFormat::Auto)]
    pub page_format: FtdiPageFormat,
}

/// Block / LBA range — read and write.
#[derive(Args, Clone, Debug)]
pub struct RangeArgs {
    /// Start block / LBA offset
    #[arg(long, default_value_t = 0)]
    pub start: u32,

    /// Number of blocks / LBAs (default: all)
    #[arg(long)]
    pub count: Option<u32>,
}

/// Extra write options.
#[derive(Args, Clone, Debug)]
pub struct WriteArgs {
    /// Erase block before writing
    #[arg(long, action = ArgAction::Set, default_value_t = true)]
    pub erase: bool,

    /// Verify block after writing
    #[arg(long)]
    pub verify: bool,
}

// ---------------------------------------------------------------------------
// Top-level CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "nandpromax")]
#[command(
    about = "Unified NAND / eMMC / XSVF flasher (PicoFlasher, FTDI, LPC, DemoN)",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub sub: Sub,
}

#[derive(Subcommand, Debug)]
pub enum Sub {
    /// NAND flash operations
    Nand {
        #[command(subcommand)]
        op: NandOp,
    },

    /// eMMC operations
    Emmc {
        #[command(subcommand)]
        op: EmmcOp,
    },

    /// XSVF / JTAG operations (LPC / JRP only)
    Xsvf {
        #[command(subcommand)]
        op: XsvfOp,
    },

    /// Display device and flash information
    Info {
        #[command(flatten)]
        device: DeviceArgs,
    },

    /// List all connected flasher hardware
    ListDevices,

    /// Bridge hardware over TCP (LPC / DemoN)
    ServeTcp {
        /// Bind address:port
        #[arg(long, default_value = "0.0.0.0:8383")]
        bind: String,

        /// Hardware device to serve
        #[arg(short = 'd', long, value_enum)]
        device: Option<DeviceType>,
    },
}

// ---------------------------------------------------------------------------
// NAND subcommands
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum NandOp {
    /// Read NAND flash to a file
    Read {
        /// Output file
        out: PathBuf,

        #[command(flatten)]
        device: DeviceArgs,

        #[command(flatten)]
        range: RangeArgs,
    },

    /// Write a file to NAND flash
    Write {
        /// Input file
        input: PathBuf,

        #[command(flatten)]
        device: DeviceArgs,

        #[command(flatten)]
        range: RangeArgs,

        #[command(flatten)]
        write: WriteArgs,
    },
}

// ---------------------------------------------------------------------------
// eMMC subcommands
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum EmmcOp {
    /// Read eMMC to a file
    Read {
        /// Output file
        out: PathBuf,

        #[command(flatten)]
        device: DeviceArgs,

        #[command(flatten)]
        range: RangeArgs,
    },

    /// Write a file to eMMC
    Write {
        /// Input file
        input: PathBuf,

        #[command(flatten)]
        device: DeviceArgs,

        #[command(flatten)]
        range: RangeArgs,

        #[command(flatten)]
        write: WriteArgs,
    },
}

// ---------------------------------------------------------------------------
// XSVF subcommands
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum XsvfOp {
    /// Detect the JTAG target device
    Detect {
        #[command(flatten)]
        device: DeviceArgs,
    },

    /// Program an XSVF file via JTAG
    Write {
        /// Input XSVF file
        input: PathBuf,

        #[command(flatten)]
        device: DeviceArgs,
    },
}
