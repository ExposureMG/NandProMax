use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

pub use crate::types::DeviceType;

// ---------------------------------------------------------------------------
// Shared argument groups (flattened into subcommands)
// ---------------------------------------------------------------------------

/// Hardware / connection options — shared by all subcommands.
#[derive(Args, Clone, Debug)]
pub struct DeviceArgs {
    /// Hardware device
    #[arg(short = 'd', long, value_enum)]
    pub device: Option<DeviceType>,

    /// Operation timeout in milliseconds (must be at least 1)
    #[arg(long, default_value_t = 3000, value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout_ms: u64,

    /// USB serial port (e.g. /dev/ttyACM0) — PicoFlasher; auto-detected if omitted
    #[arg(long)]
    pub serial: Option<String>,
}

/// Block / LBA range — read and write.
#[derive(Args, Clone, Debug)]
pub struct RangeArgs {
    /// Start block / LBA offset
    #[arg(long, default_value_t = 0)]
    pub start: u32,

    /// Number of blocks / LBAs (default: all; on write, the first N blocks of the input)
    #[arg(long)]
    pub count: Option<u32>,
}

/// Extra write options.
#[derive(Args, Clone, Debug)]
pub struct WriteArgs {
    /// Currently has no effect: the tool never issues an erase itself; whether a
    /// write erases first is up to the device firmware
    #[arg(long, action = ArgAction::Set, default_value_t = true)]
    pub erase: bool,

    /// Read each block back after writing it and compare; abort on the first mismatch
    #[arg(long)]
    pub verify: bool,
}

// ---------------------------------------------------------------------------
// Top-level CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "nandpromax")]
#[command(
    about = "Unified NAND / eMMC / XSVF flasher (PicoFlasher, LPC, DemoN)",
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
        /// Bind address:port. The server has no authentication: anyone who can
        /// connect can read and write the flash. Binding 0.0.0.0 exposes the
        /// flasher to the whole network; keep the loopback default unless you
        /// trust that network
        #[arg(long, default_value = "127.0.0.1:8383")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_must_be_positive() {
        assert!(Cli::try_parse_from(["nandpromax", "info", "--timeout-ms", "0"]).is_err());
        assert!(Cli::try_parse_from(["nandpromax", "info", "--timeout-ms", "1"]).is_ok());
    }

    #[test]
    fn serve_tcp_defaults_to_loopback() {
        let cli = Cli::try_parse_from(["nandpromax", "serve-tcp"]).unwrap();
        match cli.sub {
            Sub::ServeTcp { bind, .. } => assert_eq!(bind, "127.0.0.1:8383"),
            _ => panic!("expected serve-tcp"),
        }
    }
}
