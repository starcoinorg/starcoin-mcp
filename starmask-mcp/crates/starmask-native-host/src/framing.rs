use std::io::{ErrorKind, Read, Write};

use anyhow::{Result, bail};

use starmask_types::{NATIVE_BRIDGE_MAX_INBOUND_BYTES, NATIVE_BRIDGE_MAX_OUTBOUND_BYTES};

pub fn read_frame<R>(reader: &mut R) -> Result<Option<Vec<u8>>>
where
    R: Read,
{
    let mut header = [0_u8; 4];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }

    let length = u32::from_ne_bytes(header);
    if length > NATIVE_BRIDGE_MAX_INBOUND_BYTES {
        bail!("native bridge frame exceeds inbound limit: {length}");
    }

    let mut payload = vec![0_u8; usize::try_from(length)?];
    reader.read_exact(&mut payload)?;
    Ok(Some(payload))
}

pub fn write_frame<W>(writer: &mut W, payload: &[u8]) -> Result<()>
where
    W: Write,
{
    let payload_len = u32::try_from(payload.len())?;
    if payload_len > NATIVE_BRIDGE_MAX_OUTBOUND_BYTES {
        bail!("native bridge frame exceeds outbound limit: {payload_len}");
    }

    writer.write_all(&payload_len.to_ne_bytes())?;
    writer.write_all(payload)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{read_frame, write_frame};

    #[test]
    fn frame_round_trip() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, br#"{"type":"ping"}"#).unwrap();

        let mut cursor = Cursor::new(buffer);
        let payload = read_frame(&mut cursor).unwrap().unwrap();
        assert_eq!(payload, br#"{"type":"ping"}"#);
        assert_eq!(read_frame(&mut cursor).unwrap(), None);
    }
}
