use std::cell::Cell;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::demon::usb::UsbClient as DemonUsb;
use crate::demon::DemonClient;
use crate::flasher::{check_range, first_mismatch, run_read_nand, run_write_nand};
use crate::lpc::usb::UsbClient as LpcUsb;
use crate::lpc::{FlashConfig, LpcClient};
use crate::picoflasher::pfc::{
    Client, CMD_EMMC_DETECT, CMD_EMMC_GET_EXT_CSD, CMD_EMMC_INIT, CMD_EMMC_READ, CMD_EMMC_WRITE,
    CMD_GET_FLASH_CONFIG, CMD_GET_VERSION, CMD_READ_FLASH, CMD_SET_SMC_WORKAROUND, CMD_START_SMC,
    CMD_STOP_SMC, CMD_WRITE_FLASH, EMMC_BLOCK_BYTES, NAND_BLOCK_BYTES,
};
use crate::progress::Progress;
use crate::tcp::TcpServer;
use crate::types::{DeviceType, MediaType};

/// Format and emit a log message through a [`Progress`] sink.
macro_rules! plog {
    ($p:expr, $($arg:tt)*) => {
        $p.log(&format!($($arg)*))
    };
}

// ---------------------------------------------------------------------------
// Top-level command handlers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn cmd_read_nand(
    out: PathBuf,
    device: Option<DeviceType>,
    media_type: Option<MediaType>,
    start: u32,
    count: Option<u32>,
    serial: Option<String>,
    timeout_ms: u64,
    progress: &mut dyn Progress,
) -> Result<()> {
    let timeout = Duration::from_millis(timeout_ms);
    let (target_dev, target_media) =
        auto_detect_device(device, media_type, serial.as_deref(), timeout)?;

    plog!(
        progress,
        "Using device={:?} type={:?}",
        target_dev,
        target_media
    );

    let t0 = Instant::now();
    match target_dev {
        DeviceType::Pico => {
            let mut client = connect_pico(serial.as_deref(), timeout, progress)?;
            pico_read(
                &mut client,
                pico_media(target_media),
                &out,
                start,
                count,
                progress,
            )?;
        }
        DeviceType::Lpc => {
            let mut client = LpcClient::open().context("Failed to open LPC device")?;
            run_read_nand(&mut client, out, start, count, progress)?;
        }
        DeviceType::Demon => {
            let mut client = DemonClient::open().context("Failed to open DemoN device")?;
            run_read_nand(&mut client, out, start, count, progress)?;
        }
        DeviceType::Jrp => bail!("JR-Programmer read not yet implemented"),
    }

    plog!(
        progress,
        "Operation completed in {:.2}s",
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_write_nand(
    input: PathBuf,
    device: Option<DeviceType>,
    media_type: Option<MediaType>,
    start: u32,
    count: Option<u32>,
    erase: bool,
    verify: bool,
    serial: Option<String>,
    timeout_ms: u64,
    progress: &mut dyn Progress,
) -> Result<()> {
    // No backend issues an explicit erase; whether a write erases first is up to
    // the device firmware. The flag is kept for API/CLI compatibility.
    let _ = erase;

    let timeout = Duration::from_millis(timeout_ms);
    let (target_dev, target_media) =
        auto_detect_device(device, media_type, serial.as_deref(), timeout)?;

    plog!(
        progress,
        "Using device={:?} type={:?}",
        target_dev,
        target_media
    );

    let t0 = Instant::now();
    match target_dev {
        DeviceType::Pico => {
            let mut client = connect_pico(serial.as_deref(), timeout, progress)?;
            pico_write(
                &mut client,
                pico_media(target_media),
                &input,
                start,
                count,
                verify,
                progress,
            )?;
        }
        DeviceType::Lpc => {
            let mut client = LpcClient::open().context("Failed to open LPC device")?;
            run_write_nand(&mut client, input, start, count, verify, progress)?;
        }
        DeviceType::Demon => {
            let mut client = DemonClient::open().context("Failed to open DemoN device")?;
            run_write_nand(&mut client, input, start, count, verify, progress)?;
        }
        DeviceType::Jrp => bail!("JR-Programmer write not yet implemented"),
    }

    plog!(
        progress,
        "Operation completed in {:.2}s",
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}

pub fn cmd_info(
    device: Option<DeviceType>,
    serial: Option<String>,
    timeout_ms: u64,
    progress: &mut dyn Progress,
) -> Result<()> {
    let timeout = Duration::from_millis(timeout_ms);
    let target_dev = device.unwrap_or(DeviceType::Pico);

    match target_dev {
        DeviceType::Pico => {
            let (mut client, resolved) = {
                let port = serial.as_deref().unwrap_or("");
                Client::connect(port, timeout)?
            };
            plog!(progress, "PicoFlasher connected to {resolved}");
            let ver = client.cmd_u32(CMD_GET_VERSION, 0)?;
            plog!(progress, "PicoFlasher Firmware Version: 0x{ver:08x}");
        }
        DeviceType::Lpc => {
            lpc_info(progress)?;
        }
        DeviceType::Jrp => bail!("JR-Programmer info not yet implemented"),
        DeviceType::Demon => {
            demon_info(progress)?;
        }
    }
    Ok(())
}

pub fn cmd_list_devices(progress: &mut dyn Progress) -> Result<()> {
    plog!(progress, "Listing available devices:");
    plog!(progress, "1. Serial ports (PicoFlasher / CDC):");
    if let Ok(ports) = serialport::available_ports() {
        if ports.is_empty() {
            plog!(progress, "   No serial ports found");
        } else {
            for p in ports {
                match &p.port_type {
                    serialport::SerialPortType::UsbPort(info) => {
                        plog!(
                            progress,
                            "   - {} (VID: 0x{:04x}, PID: 0x{:04x}, Product: {})",
                            p.port_name,
                            info.vid,
                            info.pid,
                            info.product.as_deref().unwrap_or("Unknown")
                        );
                    }
                    _ => {
                        plog!(progress, "   - {}", p.port_name);
                    }
                }
            }
        }
    } else {
        plog!(progress, "   Failed to query serial ports");
    }
    plog!(progress, "2. LPC devices:");
    lpc_list(progress);
    plog!(progress, "3. DemoN devices:");
    demon_list(progress);
    Ok(())
}

pub fn cmd_xsvf_detect(device: Option<DeviceType>, progress: &mut dyn Progress) -> Result<()> {
    let target_dev = device.unwrap_or(DeviceType::Lpc);
    match target_dev {
        DeviceType::Lpc | DeviceType::Jrp => {
            let mut client = LpcClient::open().context("Failed to open LPC/XFlash device")?;
            client
                .init()
                .context("Failed to initialize LPC/XFlash device")?;
            let version = client.version.unwrap_or(0);
            plog!(progress, "LPC/XFlash Device Information:");
            plog!(progress, "  ARM Version: {version}");
            match client.flash_init() {
                Ok(config) => {
                    plog!(progress, "  Flash Config: 0x{:08X}", config.raw);
                    plog!(progress, "  Controller Type: {}", config.controller_type);
                    plog!(progress, "  Block Type: {}", config.block_type);
                    plog!(progress, "  Page Size: 0x{:X} bytes", config.page_size);
                    plog!(progress, "  Block Size: 0x{:X} bytes", config.block_size);
                    plog!(progress, "  Size Blocks: 0x{:X}", config.size_blocks);
                    plog!(
                        progress,
                        "  Full File Size: 0x{:X} bytes ({} MB)",
                        config.file_size(),
                        config.file_size() / (1024 * 1024)
                    );
                    client.flash_deinit()?;
                }
                Err(e) => {
                    plog!(progress, "  Flash init failed: {e}");
                }
            }
        }
        other => bail!("XSVF detect not supported on {:?}", other),
    }
    Ok(())
}

pub fn cmd_xsvf_write(
    input: PathBuf,
    device: Option<DeviceType>,
    progress: &mut dyn Progress,
) -> Result<()> {
    let target_dev = device.unwrap_or(DeviceType::Lpc);
    match target_dev {
        DeviceType::Lpc | DeviceType::Jrp => {
            let data =
                std::fs::read(&input).with_context(|| format!("read XSVF file {:?}", input))?;
            plog!(progress, "Loaded {} bytes from {:?}", data.len(), input);

            let mut client = LpcClient::open().context("Failed to open LPC/XFlash device")?;
            client
                .init()
                .context("Failed to initialize LPC/XFlash device")?;
            client.xsvf_init().context("XSVF init failed")?;

            plog!(progress, "Programming XSVF...");
            client.xsvf_write(&data).context("XSVF write failed")?;
            client.xsvf_execute().context("XSVF execute failed")?;
            plog!(progress, "XSVF complete");
        }
        other => bail!("XSVF write not supported on {:?}", other),
    }
    Ok(())
}

pub fn cmd_serve_tcp(
    bind: String,
    device: Option<DeviceType>,
    progress: &mut dyn Progress,
) -> Result<()> {
    let target_dev = device.unwrap_or(DeviceType::Lpc);
    plog!(
        progress,
        "Starting TCP device server using {target_dev:?} backend on {bind}..."
    );
    match target_dev {
        DeviceType::Lpc | DeviceType::Jrp => {
            let client = LpcClient::open().context("Failed to open LPC device")?;
            let mut server = TcpServer::bind(&bind, client)?;
            server.run(progress)?;
        }
        DeviceType::Demon => {
            let client = DemonClient::open().context("Failed to open DemoN device")?;
            let mut server = TcpServer::bind(&bind, client)?;
            server.run(progress)?;
        }
        DeviceType::Pico => {
            bail!("serve-tcp supports the LPC and DemoN backends only");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Device auto-detection
// ---------------------------------------------------------------------------

pub fn auto_detect_device(
    user_device: Option<DeviceType>,
    user_media: Option<MediaType>,
    serial: Option<&str>,
    timeout: Duration,
) -> Result<(DeviceType, MediaType)> {
    let media = user_media.unwrap_or(MediaType::Spi);

    if let Some(dev) = user_device {
        if dev == DeviceType::Jrp {
            bail!("JR-Programmer is not yet supported");
        }
        return Ok((dev, media));
    }

    if let Some(port) = serial {
        if Client::connect(port, timeout).is_ok() {
            return Ok((DeviceType::Pico, media));
        }
    }

    if LpcUsb::is_present() {
        return Ok((DeviceType::Lpc, media));
    }

    if DemonUsb::is_present() {
        return Ok((DeviceType::Demon, media));
    }

    Ok((DeviceType::Pico, media))
}

// ---------------------------------------------------------------------------
// PicoFlasher helpers (NAND and eMMC share one implementation)
// ---------------------------------------------------------------------------

/// Per-medium parameters of the PicoFlasher block commands.
struct PicoMedia {
    /// Medium name used in error messages.
    label: &'static str,
    /// What the device calls an address: "block" (NAND page) or "lba" (eMMC sector).
    unit: &'static str,
    read_cmd: u8,
    write_cmd: u8,
    block_bytes: usize,
    /// Progress is reported every `mask + 1` blocks.
    progress_mask: u32,
    /// Medium-specific init after SMC is stopped; returns the size in blocks if known.
    prepare: fn(&mut Client, &mut dyn Progress) -> Result<Option<u32>>,
}

const NAND_MEDIA: PicoMedia = PicoMedia {
    label: "nand",
    unit: "block",
    read_cmd: CMD_READ_FLASH,
    write_cmd: CMD_WRITE_FLASH,
    block_bytes: NAND_BLOCK_BYTES,
    progress_mask: 0x3F,
    prepare: prepare_nand,
};

const EMMC_MEDIA: PicoMedia = PicoMedia {
    label: "emmc",
    unit: "lba",
    read_cmd: CMD_EMMC_READ,
    write_cmd: CMD_EMMC_WRITE,
    block_bytes: EMMC_BLOCK_BYTES,
    progress_mask: 0x3FF,
    prepare: prepare_emmc,
};

fn pico_media(media: MediaType) -> &'static PicoMedia {
    match media {
        MediaType::Spi => &NAND_MEDIA,
        MediaType::Emmc => &EMMC_MEDIA,
    }
}

fn connect_pico(
    serial: Option<&str>,
    timeout: Duration,
    progress: &mut dyn Progress,
) -> Result<Client> {
    let (client, resolved) = Client::connect(serial.unwrap_or(""), timeout)?;
    plog!(progress, "connected to {resolved}");
    Ok(client)
}

/// Stop the console SMC, run `op`, then restart the SMC unless `op` failed
/// after setting `keep_stopped`. Writes set it once they start modifying the
/// device: after a failed or unverified write the console is deliberately left
/// held so it cannot boot a half-written image (re-run the write, or power
/// cycle the console, to recover). Reads and failures before the first write
/// always restart. The result of `op` is returned unchanged; a failed restart
/// is only logged so it cannot mask the real error. A panic inside `op`
/// leaves the SMC stopped.
fn with_smc_stopped<T>(
    client: &mut Client,
    progress: &mut dyn Progress,
    keep_stopped: &Cell<bool>,
    op: impl FnOnce(&mut Client, &mut dyn Progress) -> Result<T>,
) -> Result<T> {
    let ver = client.cmd_u32(CMD_GET_VERSION, 0).context("GET_VERSION")?;
    plog!(progress, "pfc version=0x{ver:08x}");
    let _ = client.cmd_void(CMD_STOP_SMC, 0);
    let _ = client.cmd_void(CMD_SET_SMC_WORKAROUND, 1);

    let result = op(client, progress);

    if result.is_err() && keep_stopped.get() {
        plog!(
            progress,
            "Warning: write did not complete; leaving the console SMC stopped so it does not start with a partial image"
        );
        return result;
    }
    if let Err(e) = client.cmd_void(CMD_START_SMC, 0) {
        plog!(progress, "Warning: failed to restart SMC: {e:#}");
    }
    result
}

fn prepare_nand(client: &mut Client, progress: &mut dyn Progress) -> Result<Option<u32>> {
    let flash_config = client
        .cmd_u32(CMD_GET_FLASH_CONFIG, 0)
        .context("GET_FLASH_CONFIG")?;
    plog!(progress, "flash_config=0x{flash_config:08x}");
    if flash_config == 0x00000000 || flash_config == 0xFFFFFFFF {
        plog!(
            progress,
            "Warning: Invalid or unreadable flash_config (0x{flash_config:08x}). Check power, wiring, or console type (SPI vs eMMC)."
        );
    }
    match pages_from_flash_config(flash_config) {
        Ok(pages) => Ok(Some(pages)),
        Err(e) => {
            plog!(progress, "Warning: could not determine NAND size: {e:#}");
            Ok(None)
        }
    }
}

/// Number of 0x210-byte pages on the NAND. PicoFlasher's `lba` is a page index
/// (the firmware reads one 0x200 page + 0x10 spare at `lba << 9`), so a 16 MB
/// NAND is 0x8000 pages, not 1024.
fn pages_from_flash_config(config: u32) -> Result<u32> {
    let cfg = FlashConfig::parse(config).with_context(|| {
        format!("unsupported flash_config 0x{config:08x} (eMMC console? try --media emmc)")
    })?;
    Ok((cfg.file_size() / EMMC_BLOCK_BYTES as u64) as u32)
}

fn prepare_emmc(client: &mut Client, progress: &mut dyn Progress) -> Result<Option<u32>> {
    let ret = client.cmd_u32(CMD_EMMC_INIT, 0).context("EMMC_INIT")?;
    if ret != 0 {
        bail!("EMMC_INIT failed: {ret}");
    }

    let ret = client.cmd_u8(CMD_EMMC_DETECT, 0).context("EMMC_DETECT")?;
    if ret == 0 {
        bail!("EMMC_DETECT failed (returned 0)");
    }

    let ext_csd = client
        .cmd_exact_bytes(CMD_EMMC_GET_EXT_CSD, 0, 512)
        .context("EMMC_GET_EXT_CSD")?;
    if ext_csd.len() != 512 {
        bail!("EXT_CSD mismatch: expected 512, got {}", ext_csd.len());
    }

    let sec_count = u32::from_le_bytes(ext_csd[212..216].try_into().unwrap());
    plog!(
        progress,
        "emmc sec_count={sec_count} (~{} MB)",
        sec_count / 2048
    );
    Ok(Some(sec_count))
}

/// Number of blocks a read covers: the explicit count, else everything from
/// `start` to the end of the medium. The range is validated against the size
/// when it is known.
fn plan_read(media: &PicoMedia, start: u32, count: Option<u32>, total: Option<u32>) -> Result<u32> {
    let blocks = match (count, total) {
        (Some(c), _) => c,
        (None, Some(t)) => t.saturating_sub(start),
        (None, None) => bail!(
            "{} size unknown for this flash_config; pass an explicit count",
            media.label
        ),
    };
    check_range(start, blocks, total)?;
    Ok(blocks)
}

/// Number of blocks a write covers: `--count` if given (must not exceed the
/// input), else the whole input. The range is validated against the size when
/// it is known.
fn plan_write(
    media: &PicoMedia,
    input_len: u64,
    start: u32,
    count: Option<u32>,
    total: Option<u32>,
) -> Result<u32> {
    let block_bytes = media.block_bytes as u64;
    if input_len % block_bytes != 0 {
        bail!(
            "input size must be a multiple of 0x{:x} (got 0x{input_len:x})",
            media.block_bytes
        );
    }
    let file_blocks =
        u32::try_from(input_len / block_bytes).context("input file has too many blocks")?;
    let blocks = count.unwrap_or(file_blocks);
    if blocks > file_blocks {
        bail!("--count {blocks} exceeds the {file_blocks} blocks in the input file");
    }
    check_range(start, blocks, total)?;
    Ok(blocks)
}

fn pico_read(
    client: &mut Client,
    media: &PicoMedia,
    out: &Path,
    start: u32,
    count: Option<u32>,
    progress: &mut dyn Progress,
) -> Result<()> {
    with_smc_stopped(client, progress, &Cell::new(false), |client, progress| {
        let total = (media.prepare)(client, progress)?;
        let blocks = plan_read(media, start, count, total)?;

        let f = File::create(out).context("open output")?;
        let mut f = BufWriter::with_capacity(1024 * 1024, f);

        for i in 0..blocks {
            let lba = start + i;
            let (ret, data) = client.read_with_ret(media.read_cmd, lba, media.block_bytes)?;
            if ret != 0 {
                bail!(
                    "{} read failed at {} {lba}: status {ret}",
                    media.label,
                    media.unit
                );
            }
            let data = data.context("missing data buffer")?;
            if data.len() != media.block_bytes {
                bail!(
                    "{} read mismatch at {} {lba}: expected {}, got {}",
                    media.label,
                    media.unit,
                    media.block_bytes,
                    data.len()
                );
            }
            f.write_all(&data).context("write output")?;
            let done = i + 1;
            if (done & media.progress_mask) == 0 || done == blocks {
                plog!(progress, "read {done}/{blocks} blocks");
                progress.update(done as u64, blocks as u64);
            }
        }

        f.flush().context("flush output")?;
        Ok(())
    })
}

fn pico_write(
    client: &mut Client,
    media: &PicoMedia,
    input: &Path,
    start: u32,
    count: Option<u32>,
    verify: bool,
    progress: &mut dyn Progress,
) -> Result<()> {
    let keep_stopped = Cell::new(false);
    with_smc_stopped(client, progress, &keep_stopped, |client, progress| {
        let total = (media.prepare)(client, progress)?;

        let f = File::open(input).context("open input")?;
        let input_len = f.metadata().context("stat input")?.len();
        let blocks = plan_write(media, input_len, start, count, total)?;
        let mut reader = BufReader::with_capacity(1024 * 1024, f);

        let mut buf = vec![0u8; media.block_bytes];

        let mut done = 0u32;
        while done < blocks {
            reader.read_exact(&mut buf).context("read input")?;
            keep_stopped.set(true);
            // Cannot overflow: plan_write checked start + blocks.
            let lba = start + done;

            let ret = client.write_single(media.write_cmd, lba, &buf)?;
            if ret != 0 {
                bail!("write failed at {} {lba}: {ret}", media.unit);
            }

            if verify {
                verify_chunk(client, media, lba, &buf)?;
            }

            done += 1;
            if (done & media.progress_mask) == 0 || done == blocks {
                plog!(progress, "written {done}/{blocks} blocks");
                progress.update(done as u64, blocks as u64);
            }
        }

        if verify {
            plog!(progress, "verified {blocks} blocks");
        }
        Ok(())
    })
}

/// Read back the blocks just written at `lba` and compare them with `expected`.
fn verify_chunk(client: &mut Client, media: &PicoMedia, lba: u32, expected: &[u8]) -> Result<()> {
    for (i, want) in expected.chunks_exact(media.block_bytes).enumerate() {
        let addr = lba + i as u32;
        let (ret, data) = client.read_with_ret(media.read_cmd, addr, media.block_bytes)?;
        if ret != 0 {
            bail!("verify read failed at {} {addr}: status {ret}", media.unit);
        }
        let got = data.context("missing data buffer")?;
        if let Some(off) = first_mismatch(want, &got) {
            bail!(
                "verify failed at {} {addr}: first mismatch at byte offset 0x{off:x}",
                media.unit
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// DemoN helpers
// ---------------------------------------------------------------------------

fn demon_list(progress: &mut dyn Progress) {
    if DemonUsb::is_present() {
        plog!(progress, "DemoN device found");
    } else {
        plog!(progress, "DemoN device not found");
    }
}

fn demon_info(progress: &mut dyn Progress) -> Result<()> {
    let mut client = DemonClient::open().context("Failed to open DemoN device")?;
    let info = client.init().context("Failed to initialize DemoN device")?;

    plog!(progress, "DemoN Device Information:");
    plog!(progress, "  Device ID: {:?}", info.device_id);
    plog!(
        progress,
        "  Protocol Version: 0x{:04x}",
        info.protocol_version
    );
    plog!(
        progress,
        "  Firmware Version: 0x{:04x}",
        info.firmware_version
    );
    plog!(progress, "  Flash ID: 0x{:04x}", info.nand_id);
    plog!(progress, "  Mode: {:?}", info.mode);
    if info.left_bootloader {
        plog!(
            progress,
            "  Note: device was in bootloader mode and was switched to firmware mode"
        );
    }

    if let Some(manufacturer) = client.get_manufacturer_name() {
        plog!(progress, "  Manufacturer: {}", manufacturer);
    }

    if let Some(nand_info) = client.get_nand_info() {
        plog!(progress, "  NAND Info:");
        plog!(progress, "    Name: {}", nand_info.name);
        plog!(progress, "    Page Size: {} bytes", nand_info.page_size);
        plog!(progress, "    Spare Size: {} bytes", nand_info.spare_size);
        plog!(progress, "    Chip Size: {} MiB", nand_info.chip_size);
        plog!(
            progress,
            "    Pages Per Block: {}",
            nand_info.pages_per_block
        );
        plog!(progress, "    Total Blocks: {}", nand_info.num_blocks());
        plog!(
            progress,
            "    Total File Size: {} bytes (0x{:x})",
            nand_info.file_size(),
            nand_info.file_size()
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// LPC / XFlash helpers
// ---------------------------------------------------------------------------

fn lpc_list(progress: &mut dyn Progress) {
    if LpcUsb::is_present() {
        plog!(progress, "LPC/XFlash device found");
    } else {
        plog!(progress, "LPC/XFlash device not found");
    }
}

fn lpc_info(progress: &mut dyn Progress) -> Result<()> {
    let mut client = LpcClient::open().context("Failed to open LPC/XFlash device")?;
    client
        .init()
        .context("Failed to initialize LPC/XFlash device")?;

    let version = client.version.unwrap_or(0);
    plog!(progress, "LPC/XFlash Device Information:");
    plog!(progress, "  ARM Version: {}", version);

    match client.flash_init() {
        Ok(config) => {
            plog!(progress, "  Flash Config: 0x{:08X}", config.raw);
            plog!(progress, "  Controller Type: {}", config.controller_type);
            plog!(progress, "  Block Type: {}", config.block_type);
            plog!(progress, "  Page Size: 0x{:X} bytes", config.page_size);
            plog!(progress, "  Meta Size: 0x{:X} bytes", config.meta_size);
            plog!(progress, "  Meta Type: {}", config.meta_type);
            plog!(progress, "  Block Size: 0x{:X} bytes", config.block_size);
            plog!(progress, "  Size Blocks: 0x{:X}", config.size_blocks);
            plog!(
                progress,
                "  Size Small Blocks: 0x{:X}",
                config.size_small_blocks
            );
            plog!(progress, "  File Blocks: 0x{:X}", config.file_blocks);
            plog!(
                progress,
                "  Full File Size: 0x{:X} bytes ({} MB)",
                config.file_size(),
                config.file_size() / (1024 * 1024)
            );
            client.flash_deinit()?;
        }
        Err(e) => {
            plog!(progress, "  Failed to initialize flash: {e}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_for_16mb_falcon_config() {
        // 0x00023010: 16 MB small-block NAND => 0x8000 pages => 0x1080000-byte image
        let pages = pages_from_flash_config(0x0002_3010).unwrap();
        assert_eq!(pages, 0x8000);
        assert_eq!(pages as usize * NAND_BLOCK_BYTES, 0x108_0000);
    }

    #[test]
    fn emmc_config_is_not_a_nand_size() {
        assert!(pages_from_flash_config(0xC046_2002).is_err());
    }

    #[test]
    fn plan_read_defaults_to_rest_of_device() {
        assert_eq!(
            plan_read(&NAND_MEDIA, 0x100, None, Some(0x8000)).unwrap(),
            0x7F00
        );
        assert_eq!(plan_read(&EMMC_MEDIA, 0, Some(10), Some(10)).unwrap(), 10);
    }

    #[test]
    fn plan_read_validates_explicit_count_against_size() {
        assert!(plan_read(&EMMC_MEDIA, 5, Some(10), Some(10)).is_err());
        assert!(plan_read(&EMMC_MEDIA, 10, None, Some(10)).is_err());
        assert!(plan_read(&EMMC_MEDIA, u32::MAX, Some(2), Some(10)).is_err());
    }

    #[test]
    fn plan_read_unknown_size() {
        assert!(plan_read(&NAND_MEDIA, 0, None, None).is_err());
        assert_eq!(plan_read(&NAND_MEDIA, 4, Some(3), None).unwrap(), 3);
        assert!(plan_read(&NAND_MEDIA, u32::MAX, Some(2), None).is_err());
    }

    #[test]
    fn plan_write_uses_whole_file_or_count() {
        let len = 8 * NAND_BLOCK_BYTES as u64;
        assert_eq!(plan_write(&NAND_MEDIA, len, 0, None, Some(100)).unwrap(), 8);
        assert_eq!(
            plan_write(&NAND_MEDIA, len, 2, Some(3), Some(100)).unwrap(),
            3
        );
        assert!(plan_write(&NAND_MEDIA, len, 0, Some(9), Some(100)).is_err());
    }

    #[test]
    fn plan_write_rejects_bad_sizes_and_ranges() {
        assert!(plan_write(&NAND_MEDIA, NAND_BLOCK_BYTES as u64 + 1, 0, None, None).is_err());
        let len = 8 * EMMC_BLOCK_BYTES as u64;
        assert!(plan_write(&EMMC_MEDIA, len, 95, None, Some(100)).is_err());
        assert!(plan_write(&EMMC_MEDIA, len, u32::MAX - 2, None, None).is_err());
    }
}
