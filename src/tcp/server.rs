use std::io::{BufReader, BufWriter};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};

use anyhow::{Context, Result};

use crate::flasher::NandFlasher;
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

    pub fn run(&mut self) -> Result<()> {
        let local_addr = self.listener.local_addr()?;
        eprintln!("TCP Device Server listening on {local_addr}...");

        let listener = self.listener.try_clone().context("clone listener")?;
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let peer = stream.peer_addr().ok();
                    eprintln!("Accepted client connection from {:?}", peer);
                    if let Err(e) = self.handle_client(stream) {
                        eprintln!("Client connection ended with error: {e:#}");
                    }
                }
                Err(e) => {
                    eprintln!("Failed to accept incoming connection: {e}");
                }
            }
        }
        Ok(())
    }

    fn handle_client(&mut self, stream: TcpStream) -> Result<()> {
        stream.set_nodelay(true)?;
        let mut reader = BufReader::with_capacity(64 * 1024, stream.try_clone()?);
        let mut writer = BufWriter::with_capacity(64 * 1024, stream);

        let geom = self.flasher.geometry().ok();

        loop {
            let req = match Frame::read_from(&mut reader) {
                Ok(frame) => frame,
                Err(_) => break,
            };

            if req.msg_type != PFC_MSG_REQUEST || req.payload.is_empty() {
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
                        Err(_) => vec![],
                    }
                }
                0x03 => {
                    let block_size = geom.as_ref().map(|g| g.block_size).unwrap_or(0x210);
                    if req.payload.len() >= 5 + block_size {
                        let res = self.flasher.write_block(lba, &req.payload[5..5 + block_size]);
                        let ret: u32 = if res.is_ok() { 0 } else { 1 };
                        ret.to_le_bytes().to_vec()
                    } else {
                        1u32.to_le_bytes().to_vec()
                    }
                }
                _ => vec![0u8; 4],
            };

            let resp = Frame::response(resp_payload);
            resp.write_to(&mut writer)?;
        }
        Ok(())
    }
}
