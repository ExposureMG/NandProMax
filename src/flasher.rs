use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

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

pub fn run_read_nand<F: NandFlasher>(
    flasher: &mut F,
    out: PathBuf,
    start: u32,
    count: Option<u32>,
) -> Result<Duration> {
    let geom = flasher.geometry().context("failed to get flash geometry")?;
    let blocks_to_read = count.unwrap_or(geom.total_blocks.saturating_sub(start));

    if start >= geom.total_blocks {
        bail!(
            "start block {start} out of range (total blocks {})",
            geom.total_blocks
        );
    }
    if start + blocks_to_read > geom.total_blocks {
        bail!(
            "requested range {}..{} out of range (total blocks {})",
            start,
            start + blocks_to_read,
            geom.total_blocks
        );
    }

    eprintln!(
        "NAND: {} ({} MiB), Block size: {} bytes, Total blocks: {}",
        geom.name, geom.chip_size_mb, geom.block_size, geom.total_blocks
    );
    eprintln!("Reading {} blocks from block {}", blocks_to_read, start);

    let f = File::create(out).context("open output file")?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, f);
    let mut block_buf = vec![0u8; geom.block_size];
    let t0 = Instant::now();

    for i in 0..blocks_to_read {
        let block_num = start + i;
        if (i & 0x3F) == 0 {
            eprintln!("Reading block {}/{}", i + 1, blocks_to_read);
        }
        flasher
            .read_block(block_num, &mut block_buf)
            .with_context(|| format!("read block {}", block_num))?;
        writer.write_all(&block_buf).context("write output")?;
    }

    writer.flush().context("flush output")?;
    flasher.deinit()?;
    Ok(t0.elapsed())
}

pub fn run_write_nand<F: NandFlasher>(
    flasher: &mut F,
    input: PathBuf,
    start: u32,
) -> Result<()> {
    let geom = flasher.geometry().context("failed to get flash geometry")?;

    let input_meta = std::fs::metadata(&input).context("stat input file")?;
    let input_len = input_meta.len() as usize;

    if input_len % geom.block_size != 0 {
        bail!(
            "input size (0x{input_len:x}) must be a multiple of block size (0x{:x})",
            geom.block_size
        );
    }

    let file_blocks = (input_len / geom.block_size) as u32;

    if start >= geom.total_blocks {
        bail!(
            "start block {start} out of range (total blocks {})",
            geom.total_blocks
        );
    }
    if start + file_blocks > geom.total_blocks {
        bail!(
            "requested range {}..{} out of range (total blocks {})",
            start,
            start + file_blocks,
            geom.total_blocks
        );
    }

    eprintln!(
        "NAND: {} ({} MiB), Writing {} blocks starting at block {}",
        geom.name, geom.chip_size_mb, file_blocks, start
    );

    let f = File::open(input).context("open input file")?;
    let mut reader = BufReader::with_capacity(1024 * 1024, f);
    let mut block_buf = vec![0u8; geom.block_size];

    for i in 0..file_blocks {
        let block_num = start + i;
        reader.read_exact(&mut block_buf).context("read input block")?;

        flasher
            .write_block(block_num, &block_buf)
            .with_context(|| format!("write block {}", block_num))?;

        if (i & 0x3F) == 0 {
            eprintln!("Written block {}/{}", i + 1, file_blocks);
        }
    }

    flasher.deinit()?;
    Ok(())
}
