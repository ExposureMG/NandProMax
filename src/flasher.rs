use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::demon::DemonClient;
use crate::lpc::LpcClient;
use crate::progress::Progress;

pub struct FlashGeometry {
    pub name: String,
    pub chip_size_mb: u32,
    pub block_size: usize,
    pub total_blocks: u32,
}

pub trait NandFlasher {
    fn geometry(&mut self) -> Result<FlashGeometry>;
    fn read_block(&mut self, block: u32, buf: &mut [u8]) -> Result<()>;
    fn write_block(&mut self, block: u32, buf: &[u8]) -> Result<()>;
    fn deinit(&mut self) -> Result<()> {
        Ok(())
    }
}

impl NandFlasher for DemonClient {
    fn geometry(&mut self) -> Result<FlashGeometry> {
        let _info = self.init().context("Failed to initialize DemoN device")?;
        let nand_info = self
            .get_nand_info()
            .ok_or_else(|| anyhow::anyhow!("NAND device not recognized"))?;
        Ok(FlashGeometry {
            name: nand_info.name.to_string(),
            chip_size_mb: nand_info.chip_size,
            block_size: nand_info.total_block_size() as usize,
            total_blocks: nand_info.num_blocks() as u32,
        })
    }

    fn read_block(&mut self, block: u32, buf: &mut [u8]) -> Result<()> {
        let page = u16::try_from(block)
            .with_context(|| format!("block {block} exceeds DemoN u16 range"))?;
        let _len = self
            .read_block(page, buf.len(), buf)
            .with_context(|| format!("read block {block}"))?;
        Ok(())
    }

    fn write_block(&mut self, block: u32, buf: &[u8]) -> Result<()> {
        let page = u16::try_from(block)
            .with_context(|| format!("block {block} exceeds DemoN u16 range"))?;
        self.write_block(page, buf)
            .with_context(|| format!("write block {block}"))?;
        Ok(())
    }
}

impl NandFlasher for LpcClient {
    fn geometry(&mut self) -> Result<FlashGeometry> {
        self.init()
            .context("Failed to initialize LPC/XFlash device")?;
        let version = self.version.unwrap_or(0);
        let config = self.flash_init().context("Failed to initialize flash")?;
        Ok(FlashGeometry {
            name: format!("LPC/XFlash (ARM v{version})"),
            chip_size_mb: (config.file_size() / (1024 * 1024)) as u32,
            block_size: 0x4200,
            total_blocks: config.size_small_blocks,
        })
    }

    fn read_block(&mut self, block: u32, buf: &mut [u8]) -> Result<()> {
        let (status, data) = self.flash_read(block)?;
        if crate::lpc::status::is_error(status) {
            bail!("Error reading block {block}: status=0x{status:X}");
        }
        if data.len() != buf.len() {
            bail!(
                "Block read length mismatch: expected {}, got {}",
                buf.len(),
                data.len()
            );
        }
        buf.copy_from_slice(&data);
        Ok(())
    }

    fn write_block(&mut self, block: u32, buf: &[u8]) -> Result<()> {
        let status = self.flash_write(block, buf)?;
        if crate::lpc::status::is_error(status) {
            bail!("Error writing block {block}: status=0x{status:X}");
        }
        Ok(())
    }

    fn deinit(&mut self) -> Result<()> {
        self.flash_deinit()?;
        Ok(())
    }
}

/// Validate that `start..start + count` fits in a device of `total` blocks and
/// return the exclusive end. `total == None` (size unknown) only guards against
/// `u32` overflow. `start >= total` is always an error, even for `count == 0`.
pub fn check_range(start: u32, count: u32, total: Option<u32>) -> Result<u32> {
    let end = start
        .checked_add(count)
        .with_context(|| format!("range start {start} + count {count} overflows u32"))?;
    if let Some(total) = total {
        if start >= total {
            bail!("start block {start} out of range (total blocks {total})");
        }
        if end > total {
            bail!("requested range {start}..{end} out of range (total blocks {total})");
        }
    }
    Ok(end)
}

/// Index of the first differing byte, or the shorter length if one slice is a
/// prefix of the other; `None` when equal.
pub fn first_mismatch(expected: &[u8], actual: &[u8]) -> Option<usize> {
    if expected == actual {
        return None;
    }
    Some(
        expected
            .iter()
            .zip(actual)
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| expected.len().min(actual.len())),
    )
}

pub fn run_read_nand<F: NandFlasher>(
    flasher: &mut F,
    out: PathBuf,
    start: u32,
    count: Option<u32>,
    progress: &mut dyn Progress,
) -> Result<()> {
    let geom = flasher.geometry().context("failed to get flash geometry")?;
    let result = read_blocks(flasher, &geom, out, start, count, progress);
    // Release the device even when the read failed; keep the read error if both fail.
    let deinit = flasher.deinit();
    result?;
    deinit
}

fn read_blocks<F: NandFlasher>(
    flasher: &mut F,
    geom: &FlashGeometry,
    out: PathBuf,
    start: u32,
    count: Option<u32>,
    progress: &mut dyn Progress,
) -> Result<()> {
    let blocks_to_read = count.unwrap_or(geom.total_blocks.saturating_sub(start));
    check_range(start, blocks_to_read, Some(geom.total_blocks))?;

    progress.log(&format!(
        "NAND: {} ({} MiB), Block size: {} bytes, Total blocks: {}",
        geom.name, geom.chip_size_mb, geom.block_size, geom.total_blocks
    ));
    progress.log(&format!(
        "Reading {blocks_to_read} blocks from block {start}"
    ));

    let f = File::create(out).context("open output file")?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, f);
    let mut block_buf = vec![0u8; geom.block_size];

    for i in 0..blocks_to_read {
        let block_num = start + i;
        flasher
            .read_block(block_num, &mut block_buf)
            .with_context(|| format!("read block {block_num}"))?;
        writer.write_all(&block_buf).context("write output")?;
        let done = i + 1;
        if (done & 0x3F) == 0 || done == blocks_to_read {
            progress.log(&format!("read {done}/{blocks_to_read} blocks"));
            progress.update(done as u64, blocks_to_read as u64);
        }
    }

    writer.flush().context("flush output")?;
    Ok(())
}

pub fn run_write_nand<F: NandFlasher>(
    flasher: &mut F,
    input: PathBuf,
    start: u32,
    count: Option<u32>,
    verify: bool,
    progress: &mut dyn Progress,
) -> Result<()> {
    let geom = flasher.geometry().context("failed to get flash geometry")?;
    let result = write_blocks(flasher, &geom, input, start, count, verify, progress);
    let deinit = flasher.deinit();
    result?;
    deinit
}

fn write_blocks<F: NandFlasher>(
    flasher: &mut F,
    geom: &FlashGeometry,
    input: PathBuf,
    start: u32,
    count: Option<u32>,
    verify: bool,
    progress: &mut dyn Progress,
) -> Result<()> {
    let input_len = std::fs::metadata(&input).context("stat input file")?.len();
    let block_size = geom.block_size as u64;

    if input_len % block_size != 0 {
        bail!(
            "input size (0x{input_len:x}) must be a multiple of block size (0x{:x})",
            geom.block_size
        );
    }

    let file_blocks =
        u32::try_from(input_len / block_size).context("input file has too many blocks")?;
    let blocks = count.unwrap_or(file_blocks);
    if blocks > file_blocks {
        bail!("--count {blocks} exceeds the {file_blocks} blocks in the input file");
    }
    check_range(start, blocks, Some(geom.total_blocks))?;

    progress.log(&format!(
        "NAND: {} ({} MiB), Writing {blocks} blocks starting at block {start}{}",
        geom.name,
        geom.chip_size_mb,
        if verify { " (with verify)" } else { "" }
    ));

    let f = File::open(input).context("open input file")?;
    let mut reader = BufReader::with_capacity(1024 * 1024, f);
    let mut block_buf = vec![0u8; geom.block_size];
    let mut check_buf = vec![0u8; if verify { geom.block_size } else { 0 }];

    for i in 0..blocks {
        let block_num = start + i;
        reader
            .read_exact(&mut block_buf)
            .context("read input block")?;

        flasher
            .write_block(block_num, &block_buf)
            .with_context(|| format!("write block {block_num}"))?;

        if verify {
            flasher
                .read_block(block_num, &mut check_buf)
                .with_context(|| format!("read back block {block_num} for verify"))?;
            if let Some(off) = first_mismatch(&block_buf, &check_buf) {
                bail!(
                    "verify failed at block {block_num}: first mismatch at byte offset 0x{off:x}"
                );
            }
        }

        let done = i + 1;
        if (done & 0x3F) == 0 || done == blocks {
            progress.log(&format!("written {done}/{blocks} blocks"));
            progress.update(done as u64, blocks as u64);
        }
    }

    if verify {
        progress.log(&format!("verified {blocks} blocks"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Default)]
    struct VecProgress {
        logs: Vec<String>,
        updates: Vec<(u64, u64)>,
    }

    impl Progress for VecProgress {
        fn log(&mut self, msg: &str) {
            self.logs.push(msg.to_string());
        }
        fn update(&mut self, done: u64, total: u64) {
            self.updates.push((done, total));
        }
    }

    struct MockFlasher {
        block_size: usize,
        blocks: Vec<Vec<u8>>,
        /// Block whose written data is silently corrupted.
        corrupt_block: Option<u32>,
        deinit_calls: u32,
    }

    impl MockFlasher {
        fn new(total: u32, block_size: usize) -> Self {
            Self {
                block_size,
                blocks: vec![vec![0xFF; block_size]; total as usize],
                corrupt_block: None,
                deinit_calls: 0,
            }
        }
    }

    impl NandFlasher for MockFlasher {
        fn geometry(&mut self) -> Result<FlashGeometry> {
            Ok(FlashGeometry {
                name: "mock".into(),
                chip_size_mb: 1,
                block_size: self.block_size,
                total_blocks: self.blocks.len() as u32,
            })
        }
        fn read_block(&mut self, block: u32, buf: &mut [u8]) -> Result<()> {
            buf.copy_from_slice(&self.blocks[block as usize]);
            Ok(())
        }
        fn write_block(&mut self, block: u32, buf: &[u8]) -> Result<()> {
            let mut data = buf.to_vec();
            if self.corrupt_block == Some(block) {
                data[3] ^= 0x01;
            }
            self.blocks[block as usize] = data;
            Ok(())
        }
        fn deinit(&mut self) -> Result<()> {
            self.deinit_calls += 1;
            Ok(())
        }
    }

    fn temp_file(contents: &[u8]) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "nandpromax-flasher-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn check_range_accepts_valid_ranges() {
        assert_eq!(check_range(0, 10, Some(10)).unwrap(), 10);
        assert_eq!(check_range(4, 6, Some(10)).unwrap(), 10);
        assert_eq!(check_range(4, 0, Some(10)).unwrap(), 4);
        assert_eq!(check_range(7, 100, None).unwrap(), 107);
    }

    #[test]
    fn check_range_rejects_out_of_range() {
        assert!(check_range(10, 0, Some(10)).is_err());
        assert!(check_range(11, 1, Some(10)).is_err());
        assert!(check_range(5, 6, Some(10)).is_err());
    }

    #[test]
    fn check_range_rejects_u32_overflow() {
        assert!(check_range(u32::MAX, 1, Some(u32::MAX)).is_err());
        assert!(check_range(u32::MAX - 1, 5, None).is_err());
        assert!(check_range(1, u32::MAX, None).is_err());
    }

    #[test]
    fn first_mismatch_finds_offset() {
        assert_eq!(first_mismatch(&[1, 2, 3], &[1, 2, 3]), None);
        assert_eq!(first_mismatch(&[1, 2, 3], &[1, 9, 3]), Some(1));
        assert_eq!(first_mismatch(&[1, 2, 3], &[1, 2]), Some(2));
    }

    #[test]
    fn write_honors_count_and_start() {
        let mut dev = MockFlasher::new(8, 4);
        let data: Vec<u8> = (0..16).collect(); // 4 blocks
        let path = temp_file(&data);
        let mut p = VecProgress::default();
        run_write_nand(&mut dev, path.clone(), 2, Some(2), false, &mut p).unwrap();
        std::fs::remove_file(path).ok();
        assert_eq!(dev.blocks[2], vec![0, 1, 2, 3]);
        assert_eq!(dev.blocks[3], vec![4, 5, 6, 7]);
        assert_eq!(dev.blocks[4], vec![0xFF; 4]);
        assert_eq!(p.updates.last(), Some(&(2, 2)));
        assert_eq!(dev.deinit_calls, 1);
    }

    #[test]
    fn write_rejects_count_beyond_file() {
        let mut dev = MockFlasher::new(8, 4);
        let path = temp_file(&[0u8; 8]);
        let mut p = VecProgress::default();
        let err = run_write_nand(&mut dev, path.clone(), 0, Some(3), false, &mut p).unwrap_err();
        std::fs::remove_file(path).ok();
        assert!(format!("{err:#}").contains("exceeds"));
        assert_eq!(dev.deinit_calls, 1);
    }

    #[test]
    fn write_rejects_range_past_device_end() {
        let mut dev = MockFlasher::new(4, 4);
        let path = temp_file(&[0u8; 12]);
        let mut p = VecProgress::default();
        assert!(run_write_nand(&mut dev, path.clone(), 2, None, false, &mut p).is_err());
        assert!(run_write_nand(&mut dev, path.clone(), u32::MAX, None, false, &mut p).is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn verify_reports_first_corrupt_block() {
        let mut dev = MockFlasher::new(8, 4);
        dev.corrupt_block = Some(3);
        let path = temp_file(&[0u8; 24]);
        let mut p = VecProgress::default();
        let err = run_write_nand(&mut dev, path.clone(), 1, None, true, &mut p).unwrap_err();
        std::fs::remove_file(path).ok();
        let msg = format!("{err:#}");
        assert!(msg.contains("verify failed at block 3"), "{msg}");
        assert!(msg.contains("0x3"), "{msg}");
        assert_eq!(dev.deinit_calls, 1);
    }

    #[test]
    fn verify_passes_on_clean_write() {
        let mut dev = MockFlasher::new(8, 4);
        let path = temp_file(&[7u8; 16]);
        let mut p = VecProgress::default();
        run_write_nand(&mut dev, path.clone(), 0, None, true, &mut p).unwrap();
        std::fs::remove_file(path).ok();
        assert!(p.logs.iter().any(|l| l.contains("verified 4 blocks")));
    }

    #[test]
    fn read_roundtrips_and_checks_range() {
        let mut dev = MockFlasher::new(4, 4);
        dev.blocks[1] = vec![1, 2, 3, 4];
        let out = std::env::temp_dir().join(format!("nandpromax-read-test-{}", std::process::id()));
        let mut p = VecProgress::default();
        run_read_nand(&mut dev, out.clone(), 1, Some(2), &mut p).unwrap();
        let got = std::fs::read(&out).unwrap();
        std::fs::remove_file(&out).ok();
        assert_eq!(got.len(), 8);
        assert_eq!(&got[..4], &[1, 2, 3, 4]);
        assert!(run_read_nand(&mut dev, out.clone(), 1, Some(u32::MAX), &mut p).is_err());
        assert!(run_read_nand(&mut dev, out, 4, None, &mut p).is_err());
    }
}
