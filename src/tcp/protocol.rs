use std::io::{Read, Write};

use anyhow::{bail, Context, Result};

pub const PFC_MAGIC: u32 = 0x5046_4331;
pub const PFC_VERSION: u16 = 1;

pub const PFC_MSG_REQUEST: u16 = 0;
pub const PFC_MSG_RESPONSE: u16 = 1;

/// Upper bound on a frame payload. The largest legitimate frame carries one
/// backend block (0x4200 bytes for LPC), so 1 MiB is generous while still
/// preventing an untrusted length header from forcing a huge allocation.
pub const MAX_PAYLOAD: usize = 1024 * 1024;

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
        if self.payload.len() > MAX_PAYLOAD {
            bail!(
                "tcp payload too large: {} bytes (max {MAX_PAYLOAD})",
                self.payload.len()
            );
        }

        let mut hdr = [0u8; 12];
        hdr[0..4].copy_from_slice(&PFC_MAGIC.to_le_bytes());
        hdr[4..6].copy_from_slice(&PFC_VERSION.to_le_bytes());
        hdr[6..8].copy_from_slice(&self.msg_type.to_le_bytes());
        hdr[8..12].copy_from_slice(&(self.payload.len() as u32).to_le_bytes());

        w.write_all(&hdr).context("tcp write header failed")?;
        if !self.payload.is_empty() {
            w.write_all(&self.payload)
                .context("tcp write payload failed")?;
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

        if len > MAX_PAYLOAD {
            bail!("tcp payload too large: {len} bytes (max {MAX_PAYLOAD})");
        }

        let mut payload = vec![0u8; len];
        if len != 0 {
            r.read_exact(&mut payload)
                .context("tcp read payload failed")?;
        }

        Ok(Self { msg_type, payload })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn header(magic: u32, version: u16, msg_type: u16, len: u32) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(&magic.to_le_bytes());
        h.extend_from_slice(&version.to_le_bytes());
        h.extend_from_slice(&msg_type.to_le_bytes());
        h.extend_from_slice(&len.to_le_bytes());
        h
    }

    #[test]
    fn roundtrip() {
        let frame = Frame::request(vec![0x02, 1, 2, 3, 4]);
        let mut buf = Vec::new();
        frame.write_to(&mut buf).unwrap();
        assert_eq!(buf.len(), 12 + 5);

        let got = Frame::read_from(&mut Cursor::new(buf)).unwrap();
        assert_eq!(got.msg_type, PFC_MSG_REQUEST);
        assert_eq!(got.payload, vec![0x02, 1, 2, 3, 4]);
    }

    #[test]
    fn roundtrip_empty_response() {
        let mut buf = Vec::new();
        Frame::response(Vec::new()).write_to(&mut buf).unwrap();
        assert_eq!(buf.len(), 12);

        let got = Frame::read_from(&mut Cursor::new(buf)).unwrap();
        assert_eq!(got.msg_type, PFC_MSG_RESPONSE);
        assert!(got.payload.is_empty());
    }

    #[test]
    fn roundtrip_max_payload() {
        let mut buf = Vec::new();
        Frame::request(vec![0xA5; MAX_PAYLOAD])
            .write_to(&mut buf)
            .unwrap();
        let got = Frame::read_from(&mut Cursor::new(buf)).unwrap();
        assert_eq!(got.payload.len(), MAX_PAYLOAD);
    }

    #[test]
    fn bad_magic_rejected() {
        let buf = header(0xDEAD_BEEF, PFC_VERSION, PFC_MSG_REQUEST, 0);
        let err = Frame::read_from(&mut Cursor::new(buf)).unwrap_err();
        assert!(err.to_string().contains("bad magic"), "{err:#}");
    }

    #[test]
    fn bad_version_rejected() {
        let buf = header(PFC_MAGIC, PFC_VERSION + 1, PFC_MSG_REQUEST, 0);
        let err = Frame::read_from(&mut Cursor::new(buf)).unwrap_err();
        assert!(err.to_string().contains("unsupported version"), "{err:#}");
    }

    #[test]
    fn oversize_length_rejected_before_reading_payload() {
        // The stream carries no payload at all: if the length were trusted we
        // would allocate ~4 GiB and then fail with a read error instead.
        let buf = header(PFC_MAGIC, PFC_VERSION, PFC_MSG_REQUEST, u32::MAX);
        let err = Frame::read_from(&mut Cursor::new(buf)).unwrap_err();
        assert!(err.to_string().contains("too large"), "{err:#}");

        let buf = header(
            PFC_MAGIC,
            PFC_VERSION,
            PFC_MSG_REQUEST,
            MAX_PAYLOAD as u32 + 1,
        );
        let err = Frame::read_from(&mut Cursor::new(buf)).unwrap_err();
        assert!(err.to_string().contains("too large"), "{err:#}");
    }

    #[test]
    fn oversize_write_rejected() {
        let mut buf = Vec::new();
        let err = Frame::request(vec![0; MAX_PAYLOAD + 1])
            .write_to(&mut buf)
            .unwrap_err();
        assert!(err.to_string().contains("too large"), "{err:#}");
        assert!(
            buf.is_empty(),
            "nothing may be written for a rejected frame"
        );
    }

    #[test]
    fn truncated_payload_is_an_error() {
        let mut buf = header(PFC_MAGIC, PFC_VERSION, PFC_MSG_RESPONSE, 8);
        buf.extend_from_slice(&[1, 2, 3]);
        assert!(Frame::read_from(&mut Cursor::new(buf)).is_err());
    }
}
