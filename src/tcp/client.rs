use std::io::{BufReader, BufWriter};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};

use crate::flasher::{FlashGeometry, NandFlasher};
use crate::tcp::protocol::{Frame, PFC_MSG_RESPONSE};

pub struct TcpClient {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

impl TcpClient {
    pub fn connect<A: ToSocketAddrs>(addr: A, timeout: Duration) -> Result<(Self, SocketAddr)> {
        let mut last_err: Option<anyhow::Error> = None;
        let mut resolved: Option<SocketAddr> = None;

        for sock in addr.to_socket_addrs().context("failed to resolve address")? {
            resolved = Some(sock);
            match TcpStream::connect_timeout(&sock, timeout) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(timeout))
                        .context("failed to set read timeout")?;
                    stream
                        .set_write_timeout(Some(timeout))
                        .context("failed to set write timeout")?;
                    stream
                        .set_nodelay(true)
                        .context("failed to set nodelay")?;
                    let reader = BufReader::with_capacity(64 * 1024, stream.try_clone().context("clone stream")?);
                    let writer = BufWriter::with_capacity(64 * 1024, stream);
                    return Ok((Self { reader, writer }, sock));
                }
                Err(e) => last_err = Some(anyhow!(e).context(format!("connect to {sock} failed"))),
            }
        }

        match (resolved, last_err) {
            (Some(_), Some(e)) => Err(e),
            _ => bail!("no socket addresses found"),
        }
    }

    pub fn send_request(&mut self, payload: &[u8]) -> Result<()> {
        let frame = Frame::request(payload.to_vec());
        frame.write_to(&mut self.writer)
    }

    pub fn recv_response(&mut self) -> Result<Frame> {
        let frame = Frame::read_from(&mut self.reader)?;
        if frame.msg_type != PFC_MSG_RESPONSE {
            bail!("unexpected message type {}, expected response", frame.msg_type);
        }
        Ok(frame)
    }

    pub fn request_response(&mut self, payload: &[u8]) -> Result<Frame> {
        self.send_request(payload)?;
        self.recv_response()
    }

    pub fn cmd_u32(&mut self, cmd: u8, lba: u32) -> Result<u32> {
        let mut payload = [0u8; 5];
        payload[0] = cmd;
        payload[1..5].copy_from_slice(&lba.to_le_bytes());
        let frame = self.request_response(&payload)?;
        if frame.payload.len() != 4 {
            bail!("expected 4-byte response, got {}", frame.payload.len());
        }
        Ok(u32::from_le_bytes(frame.payload[..4].try_into().unwrap()))
    }
}

impl NandFlasher for TcpClient {
    fn geometry(&mut self) -> Result<FlashGeometry> {
        let _ver = self.cmd_u32(0x00, 0)?;
        let flash_config = self.cmd_u32(0x01, 0)?;
        let total_blocks = match (flash_config >> 17) & 0x03 {
            0 => 1024,
            1 => 2048,
            2 => 4096,
            _ => 1024,
        };
        Ok(FlashGeometry {
            name: format!("TCP Flasher (Config 0x{flash_config:08x})"),
            chip_size_mb: total_blocks * 16 / 1024,
            block_size: 0x210,
            total_blocks,
        })
    }

    fn read_block(&mut self, block: u32, buf: &mut [u8]) -> Result<()> {
        let mut payload = [0u8; 5];
        payload[0] = 0x02;
        payload[1..5].copy_from_slice(&block.to_le_bytes());
        let frame = self.request_response(&payload)?;
        if frame.payload.len() != buf.len() {
            bail!(
                "TCP read mismatch: expected {} bytes, got {}",
                buf.len(),
                frame.payload.len()
            );
        }
        buf.copy_from_slice(&frame.payload);
        Ok(())
    }

    fn write_block(&mut self, block: u32, buf: &[u8]) -> Result<()> {
        let mut payload = Vec::with_capacity(5 + buf.len());
        payload.push(0x03);
        payload.extend_from_slice(&block.to_le_bytes());
        payload.extend_from_slice(buf);
        let frame = self.request_response(&payload)?;
        if frame.payload.len() < 4 {
            bail!("short write response from TCP server");
        }
        let ret = u32::from_le_bytes(frame.payload[..4].try_into().unwrap());
        if ret != 0 {
            bail!("TCP write block {block} failed with code {ret}");
        }
        Ok(())
    }
}
