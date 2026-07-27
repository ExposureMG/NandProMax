use std::io::{Read, Write};

use anyhow::{bail, Context, Result};

pub const PFC_MAGIC: u32 = 0x5046_4331;
pub const PFC_VERSION: u16 = 1;

pub const PFC_MSG_REQUEST: u16 = 0;
pub const PFC_MSG_RESPONSE: u16 = 1;

#[derive(Debug, Clone)]
pub struct Frame {
    pub msg_type: u16,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn request(payload: Vec<u8>) -> Self {
        Self {
            msg_type: PFC_MSG_REQUEST,
            payload,
        }
    }

    pub fn response(payload: Vec<u8>) -> Self {
        Self {
            msg_type: PFC_MSG_RESPONSE,
            payload,
        }
    }

    pub fn write_to<W: Write>(&self, w: &mut W) -> Result<()> {
        let mut hdr = [0u8; 12];
        hdr[0..4].copy_from_slice(&PFC_MAGIC.to_le_bytes());
        hdr[4..6].copy_from_slice(&PFC_VERSION.to_le_bytes());
        hdr[6..8].copy_from_slice(&self.msg_type.to_le_bytes());
        hdr[8..12].copy_from_slice(&(self.payload.len() as u32).to_le_bytes());

        w.write_all(&hdr).context("tcp write header failed")?;
        if !self.payload.is_empty() {
            w.write_all(&self.payload).context("tcp write payload failed")?;
        }
        w.flush().context("tcp flush failed")?;
        Ok(())
    }

    pub fn read_from<R: Read>(r: &mut R) -> Result<Self> {
        let mut hdr = [0u8; 12];
        r.read_exact(&mut hdr).context("tcp read header failed")?;

        let magic = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
        let version = u16::from_le_bytes(hdr[4..6].try_into().unwrap());
        let msg_type = u16::from_le_bytes(hdr[6..8].try_into().unwrap());
        let len = u32::from_le_bytes(hdr[8..12].try_into().unwrap()) as usize;

        if magic != PFC_MAGIC {
            bail!("bad magic 0x{magic:08x}");
        }
        if version != PFC_VERSION {
            bail!("unsupported version {version}");
        }

        let mut payload = vec![0u8; len];
        if len != 0 {
            r.read_exact(&mut payload).context("tcp read payload failed")?;
        }

        Ok(Self { msg_type, payload })
    }
}

pub fn cmd_payload(cmd: u8, lba: u32) -> [u8; 5] {
    let mut buf = [0u8; 5];
    buf[0] = cmd;
    buf[1..5].copy_from_slice(&lba.to_le_bytes());
    buf
}
