use std::io::{ErrorKind, Read, Write};

use anyhow::{Result, bail};

use starmask_types::{NATIVE_BRIDGE_MAX_INBOUND_BYTES, NATIVE_BRIDGE_MAX_OUTBOUND_BYTES};

pub fn read_frame<R>(reader: &mut R) -> Result<Option<Vec<u8>>>
where
    R: Read,
{
    let mut header = [0_u8; 4];
    let mut header_read = 0;
    while header_read < header.len() {
        match reader.read(&mut header[header_read..]) {
            Ok(0) if header_read == 0 => return Ok(None),
            Ok(0) => bail!("native bridge frame header truncated after {header_read} bytes"),
            Ok(read) => header_read += read,
            Err(error) if error.kind() == ErrorKind::UnexpectedEof && header_read == 0 => {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        }
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

    use starmask_types::{NATIVE_BRIDGE_MAX_INBOUND_BYTES, NATIVE_BRIDGE_MAX_OUTBOUND_BYTES};

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

    #[test]
    fn read_frame_rejects_truncated_header() {
        let mut cursor = Cursor::new(vec![1_u8, 2, 3]);
        let error = read_frame(&mut cursor).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("native bridge frame header truncated after 3 bytes")
        );
    }

    #[test]
    fn read_frame_rejects_oversized_payload() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(NATIVE_BRIDGE_MAX_INBOUND_BYTES + 1).to_ne_bytes());
        let mut cursor = Cursor::new(bytes);
        let error = read_frame(&mut cursor).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("native bridge frame exceeds inbound limit")
        );
    }

    #[test]
    fn write_frame_rejects_oversized_payload() {
        let mut buffer = Vec::new();
        let payload = vec![0_u8; usize::try_from(NATIVE_BRIDGE_MAX_OUTBOUND_BYTES).unwrap() + 1];
        let error = write_frame(&mut buffer, &payload).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("native bridge frame exceeds outbound limit")
        );
    }
}
