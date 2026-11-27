//! Enquadramento TCP: `u32 BE tamanho || bytes`. Preâmbulo: "RCLP" || versão || modo.

use crate::crypto::{Error, Result};
use std::io::{Read, Write};

pub const MAGIC: &[u8; 4] = b"RCLP";
pub const VERSION: u8 = 1;
pub const MODE_PAIR: u8 = 1;
pub const MODE_SESSION: u8 = 2;
/// Maior quadro cifrado aceito (cabe um pedaço de blob e JSON grande de texto).
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

pub fn write_preamble<W: Write>(w: &mut W, mode: u8) -> Result<()> {
    let mut b = [0u8; 6];
    b[..4].copy_from_slice(MAGIC);
    b[4] = VERSION;
    b[5] = mode;
    w.write_all(&b)?;
    Ok(())
}

pub fn read_preamble<R: Read>(r: &mut R) -> Result<u8> {
    let mut b = [0u8; 6];
    r.read_exact(&mut b)?;
    if &b[..4] != MAGIC || b[4] != VERSION {
        return Err(Error::Proto("preâmbulo inválido".into()));
    }
    Ok(b[5])
}

pub fn write_frame<W: Write>(w: &mut W, data: &[u8]) -> Result<()> {
    if data.len() > MAX_FRAME {
        return Err(Error::Proto("quadro grande demais".into()));
    }
    let mut buf = Vec::with_capacity(4 + data.len());
    buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
    buf.extend_from_slice(data);
    w.write_all(&buf)?; // um único write: evita pacotes pequenos separados
    w.flush()?;
    Ok(())
}

pub fn read_frame<R: Read>(r: &mut R, max: usize) -> Result<Vec<u8>> {
    let mut l = [0u8; 4];
    r.read_exact(&mut l)?;
    let n = u32::from_be_bytes(l) as usize;
    if n > max {
        return Err(Error::Proto("quadro excede o limite".into()));
    }
    let mut v = vec![0u8; n];
    r.read_exact(&mut v)?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_roundtrip_and_limit() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"ola").unwrap();
        assert_eq!(read_frame(&mut Cursor::new(&buf), 100).unwrap(), b"ola");
        assert!(read_frame(&mut Cursor::new(&buf), 2).is_err());
    }

    #[test]
    fn preamble_rejects_foreign_traffic() {
        assert!(read_preamble(&mut Cursor::new(b"GET / HTTP/1.1")).is_err());
        let mut b = Vec::new();
        write_preamble(&mut b, MODE_PAIR).unwrap();
        assert_eq!(read_preamble(&mut Cursor::new(&b)).unwrap(), MODE_PAIR);
    }
}
