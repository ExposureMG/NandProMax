//! TCP "device server": exposes a local [`NandFlasher`] (DemoN, LPC/XFlash, ...)
//! over the framed TCP protocol in [`crate::tcp::protocol`].
//!
//! # Wire format
//!
//! Each message is a 12-byte header (magic `PFC1`, version, message type,
//! payload length, all little-endian) followed by the payload. Requests carry
//! `cmd: u8, lba: u32` followed by optional data.
//!
//! # Commands
//!
//! - 0x00 GET_VERSION: 4-byte `1`.
//! - 0x01 GET_FLASH_CONFIG: one of three synthetic constants derived from
//!   `total_blocks` (0x0, 0x20000, 0x40000); not a real controller config.
//! - 0x02 READ_FLASH: the bare backend block (no status prefix), or an empty
//!   payload on error. `lba` is passed straight to `NandFlasher::read_block`,
//!   so its unit is the backend's native block (e.g. 0x4200 bytes for LPC).
//! - 0x03 WRITE_FLASH: `block_size` bytes of data; replies with a 4-byte status
//!   (0 = ok).
//! - anything else: not implemented; gets a 4-byte zero reply.
//!
//! This is not the PicoFlasher wire protocol, and this crate has no client for
//! it. The server has no authentication; see `serve-tcp --bind`.

use std::io::{BufReader, BufWriter};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};

use anyhow::{Context, Result};

use crate::flasher::NandFlasher;
use crate::progress::Progress;
use crate::tcp::protocol::{Frame, PFC_MSG_REQUEST};

pub struct TcpServer<F: NandFlasher> {
    listener: TcpListener,
    flasher: F,
}

impl<F: NandFlasher> TcpServer<F> {
    pub fn bind<A: ToSocketAddrs>(addr: A, flasher: F) -> Result<Self> {
        let listener = TcpListener::bind(addr).context("failed to bind TCP listener")?;
        Ok(Self { listener, flasher })
    }

    pub fn run(&mut self, progress: &mut dyn Progress) -> Result<()> {
        let local_addr = self.listener.local_addr()?;
        progress.log(&format!("TCP Device Server listening on {local_addr}..."));

        let listener = self.listener.try_clone().context("clone listener")?;
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let peer = stream.peer_addr().ok();
                    progress.log(&format!("Accepted client connection from {peer:?}"));
                    if let Err(e) = self.handle_client(stream, progress) {
                        progress.log(&format!("Client connection ended with error: {e:#}"));
                    }
                }
                Err(e) => {
                    progress.log(&format!("Failed to accept incoming connection: {e}"));
                }
            }
        }
        Ok(())
    }

    fn handle_client(&mut self, stream: TcpStream, progress: &mut dyn Progress) -> Result<()> {
        stream.set_nodelay(true)?;
        let mut reader = BufReader::with_capacity(64 * 1024, stream.try_clone()?);
        let mut writer = BufWriter::with_capacity(64 * 1024, stream);

        let geom = self.flasher.geometry().ok();

        loop {
            let req = match Frame::read_from(&mut reader) {
                Ok(frame) => frame,
                Err(e) => {
                    progress.log(&format!("Client connection closed: {e:#}"));
                    break;
                }
            };

            if req.msg_type != PFC_MSG_REQUEST || req.payload.is_empty() {
                progress.log(&format!(
                    "Ignoring frame (type {}, {} byte payload): expected non-empty request",
                    req.msg_type,
                    req.payload.len()
                ));
                continue;
            }

            let cmd = req.payload[0];
            let lba = if req.payload.len() >= 5 {
                u32::from_le_bytes(req.payload[1..5].try_into().unwrap())
            } else {
                0
            };

            let resp_payload = match cmd {
                0x00 => vec![0x01, 0x00, 0x00, 0x00],
                0x01 => {
                    let cfg: u32 = if let Some(g) = &geom {
                        if g.total_blocks == 1024 {
                            0x0000_0000
                        } else if g.total_blocks == 2048 {
                            0x0002_0000
                        } else {
                            0x0004_0000
                        }
                    } else {
                        0x0000_0000
                    };
                    cfg.to_le_bytes().to_vec()
                }
                0x02 => {
                    let block_size = geom.as_ref().map(|g| g.block_size).unwrap_or(0x210);
                    let mut block_buf = vec![0u8; block_size];
                    match self.flasher.read_block(lba, &mut block_buf) {
                        Ok(()) => block_buf,
                        Err(e) => {
                            progress.log(&format!("read_block({lba}) failed: {e:#}"));
                            vec![]
                        }
                    }
                }
                0x03 => {
                    let block_size = geom.as_ref().map(|g| g.block_size).unwrap_or(0x210);
                    if req.payload.len() >= 5 + block_size {
                        let res = self
                            .flasher
                            .write_block(lba, &req.payload[5..5 + block_size]);
                        if let Err(e) = &res {
                            progress.log(&format!("write_block({lba}) failed: {e:#}"));
                        }
                        let ret: u32 = if res.is_ok() { 0 } else { 1 };
                        ret.to_le_bytes().to_vec()
                    } else {
                        progress.log(&format!(
                            "write_block({lba}) rejected: payload {} bytes, need {}",
                            req.payload.len(),
                            5 + block_size
                        ));
                        1u32.to_le_bytes().to_vec()
                    }
                }
                _ => {
                    progress.log(&format!(
                        "Unsupported command 0x{cmd:02x}; replying with zero status"
                    ));
                    vec![0u8; 4]
                }
            };

            let resp = Frame::response(resp_payload);
            resp.write_to(&mut writer)?;
        }
        Ok(())
    }
}
