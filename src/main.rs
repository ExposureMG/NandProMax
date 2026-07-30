mod commands;
mod demon;
mod flasher;
mod ftdi;
mod interface;
mod lpc;
mod picoflasher;
mod progress;
mod tcp;
mod types;

use anyhow::Result;
use clap::Parser;

use crate::interface::cli::{Cli, EmmcOp, NandOp, Sub, XsvfOp};
use crate::progress::StderrProgress;
use crate::types::MediaType;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let p = &mut StderrProgress;

    match cli.sub {
        Sub::Nand { op } => match op {
            NandOp::Read { out, device, range } => {
                commands::cmd_read_nand(
                    out,
                    device.device,
                    Some(MediaType::Spi),
                    range.start,
                    range.count,
                    device.serial,
                    device.addr,
                    device.ftdi_desc,
                    device.ftdi_index,
                    device.freq_hz,
                    device.page_format,
                    device.timeout_ms,
                    p,
                )?;
            }
            NandOp::Write { input, device, range, write } => {
                commands::cmd_write_nand(
                    input,
                    device.device,
                    Some(MediaType::Spi),
                    range.start,
                    range.count,
                    write.erase,
                    write.verify,
                    device.serial,
                    device.addr,
                    device.ftdi_desc,
                    device.ftdi_index,
                    device.freq_hz,
                    device.page_format,
                    device.timeout_ms,
                    p,
                )?;
            }
        },

        Sub::Emmc { op } => match op {
            EmmcOp::Read { out, device, range } => {
                commands::cmd_read_nand(
                    out,
                    device.device,
                    Some(MediaType::Emmc),
                    range.start,
                    range.count,
                    device.serial,
                    device.addr,
                    device.ftdi_desc,
                    device.ftdi_index,
                    device.freq_hz,
                    device.page_format,
                    device.timeout_ms,
                    p,
                )?;
            }
            EmmcOp::Write { input, device, range, write } => {
                commands::cmd_write_nand(
                    input,
                    device.device,
                    Some(MediaType::Emmc),
                    range.start,
                    range.count,
                    write.erase,
                    write.verify,
                    device.serial,
                    device.addr,
                    device.ftdi_desc,
                    device.ftdi_index,
                    device.freq_hz,
                    device.page_format,
                    device.timeout_ms,
                    p,
                )?;
            }
        },

        Sub::Xsvf { op } => match op {
            XsvfOp::Detect { device } => {
                commands::cmd_xsvf_detect(device.device, p)?;
            }
            XsvfOp::Write { input, device } => {
                commands::cmd_xsvf_write(input, device.device, p)?;
            }
        },

        Sub::Info { device } => {
            commands::cmd_info(
                device.device,
                device.serial,
                device.addr,
                device.ftdi_desc,
                device.ftdi_index,
                device.freq_hz,
                device.timeout_ms,
                p,
            )?;
        }

        Sub::ListDevices => {
            commands::cmd_list_devices(p)?;
        }

        Sub::ServeTcp { bind, device } => {
            commands::cmd_serve_tcp(bind, device, p)?;
        }
    }

    Ok(())
}