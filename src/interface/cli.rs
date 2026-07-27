use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "picoclient")]
#[command(about = "PicoFlasher client (TCP or USB serial)", long_about = None)]
pub struct Cli {
    #[arg(long = "ip", alias = "addr", default_value = "192.168.4.1:3232")]
    pub addr: String,

    #[arg(long)]
    pub serial: Option<String>,

    #[arg(long, default_value = "3000")]
    pub timeout_ms: u64,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum FtdiPageFormat {
	Auto,
	Small,
	Big,
}

#[derive(Subcommand, Debug)]
pub enum Command {
	ListSerial,

	ReadPost {
		#[arg(long)]
		out: Option<PathBuf>,

		#[arg(long)]
		count: Option<u64>,

		#[arg(long, default_value_t = 115200)]
		baud: u32,

		#[arg(long)]
		quiet: bool,
	},

	ReadNand {
		#[arg(long)]
		out: PathBuf,

        #[arg(long, default_value_t = 0)]
        start: u32,

        #[arg(long)]
        count: Option<u32>,
    },

    WriteNand {
        #[arg(long)]
        input: PathBuf,

        #[arg(long, default_value_t = 0)]
        start: u32,
    },

    ReadEmmc {
        #[arg(long)]
        out: PathBuf,

        #[arg(long, default_value_t = 0)]
        start: u32,

        #[arg(long)]
        count: Option<u32>,
    },

    WriteEmmc {
        #[arg(long)]
        input: PathBuf,

        #[arg(long, default_value_t = 0)]
        start: u32,
    },

    FtdiReadNand {
        #[arg(long)]
        out: PathBuf,

        #[arg(long, default_value_t = 0)]
        start: u32,

        #[arg(long)]
        count: Option<u32>,

		#[arg(long, value_enum, default_value_t = FtdiPageFormat::Auto)]
		page_format: FtdiPageFormat,

		#[arg(long, default_value = "auto")]
		ftdi_desc: String,

        #[arg(long)]
        ftdi_index: Option<i32>,

        #[arg(long, default_value_t = 30_000_000)]
        freq_hz: u32,
    },

    FtdiWriteNand {
        #[arg(long)]
        input: PathBuf,

        #[arg(long, default_value_t = 0)]
        start: u32,

        #[arg(long)]
        count: Option<u32>,

		#[arg(long, value_enum, default_value_t = FtdiPageFormat::Auto)]
		page_format: FtdiPageFormat,

		#[arg(long, default_value = "auto")]
		ftdi_desc: String,

        #[arg(long)]
        ftdi_index: Option<i32>,

		#[arg(long, default_value_t = 30_000_000)]
		freq_hz: u32,

		#[arg(long, action = ArgAction::Set, default_value_t = true)]
		erase: bool,

		#[arg(long)]
		verify: bool,
	},

	FtdiReadPost {
		#[arg(long)]
		out: Option<PathBuf>,

		#[arg(long)]
		count: Option<u64>,

		#[arg(long, default_value = "auto")]
		ftdi_desc: String,

		#[arg(long)]
		ftdi_index: Option<i32>,

		#[arg(long)]
		quiet: bool,

		#[arg(long, default_value_t = 1000)]
		poll_us: u64,
	},

    FtdiList,
    DemonReadNand {
        #[arg(long)]
        out: PathBuf,

        #[arg(long, default_value_t = 0)]
        start: u32,

        #[arg(long)]
        count: Option<u32>,
    },

    DemonWriteNand {
        #[arg(long)]
        input: PathBuf,

        #[arg(long, default_value_t = 0)]
        start: u32,
    },

    DemonInfo,
    DemonList,
    // LPC/XFlash commands
    LpcInfo,
    LpcList,
    LpcReadNand {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 0)]
        start: u32,
        #[arg(long)]
        count: Option<u32>,
    },
    LpcWriteNand {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value_t = 0)]
        start: u32,
    },
}
