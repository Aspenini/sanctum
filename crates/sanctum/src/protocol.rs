use std::io::{self, Read, Write};

pub const VERSION: u16 = 1;
pub const MAX_PAYLOAD: usize = 2 * 1024 * 1024;

pub const HELLO: u8 = 1;
pub const STATUS: u8 = 2;
pub const FRAME: u8 = 3;
pub const MENU: u8 = 4;
pub const LOG: u8 = 5;
pub const ERROR: u8 = 6;
pub const EXITED: u8 = 7;
pub const KEY: u8 = 101;
pub const STOP: u8 = 102;
pub const MUTE: u8 = 103;
pub const MOUSE: u8 = 104;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    pub character: i64,
    pub scan_code: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn write_message(mut writer: impl Write, kind: u8, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "IPC payload is too large",
        ));
    }
    writer.write_all(&[kind])?;
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

pub fn read_message(mut reader: impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0_u8; 5];
    reader.read_exact(&mut header)?;
    let size = u32::from_le_bytes(header[1..5].try_into().unwrap()) as usize;
    if size > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC payload is too large",
        ));
    }
    let mut payload = vec![0; size];
    reader.read_exact(&mut payload)?;
    Ok((header[0], payload))
}

pub fn encode_key(ch: i64, scan: i64) -> [u8; 16] {
    let mut payload = [0; 16];
    payload[..8].copy_from_slice(&ch.to_le_bytes());
    payload[8..].copy_from_slice(&scan.to_le_bytes());
    payload
}

pub fn decode_key(payload: &[u8]) -> Option<(i64, i64)> {
    Some((
        i64::from_le_bytes(payload.get(..8)?.try_into().ok()?),
        i64::from_le_bytes(payload.get(8..16)?.try_into().ok()?),
    ))
}

pub fn encode_key_event(event: KeyEvent) -> [u8; 16] {
    encode_key(event.character, event.scan_code)
}

pub fn decode_key_event(payload: &[u8]) -> Option<KeyEvent> {
    decode_key(payload).map(|(character, scan_code)| KeyEvent {
        character,
        scan_code,
    })
}

pub fn encode_mouse(x: i64, y: i64, left: bool, right: bool) -> [u8; 17] {
    let mut payload = [0; 17];
    payload[..8].copy_from_slice(&x.to_le_bytes());
    payload[8..16].copy_from_slice(&y.to_le_bytes());
    payload[16] = u8::from(left) | (u8::from(right) << 1);
    payload
}

pub fn decode_mouse(payload: &[u8]) -> Option<(i64, i64, bool, bool)> {
    Some((
        i64::from_le_bytes(payload.get(..8)?.try_into().ok()?),
        i64::from_le_bytes(payload.get(8..16)?.try_into().ok()?),
        payload.get(16)? & 1 != 0,
        payload.get(16)? & 2 != 0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framed_messages_survive_fragmented_reads() {
        let mut bytes = Vec::new();
        write_message(&mut bytes, STATUS, b"running").unwrap();
        let mut slow = bytes.chunks(2).flat_map(|chunk| chunk.iter().copied());
        let mut rebuilt = Vec::new();
        rebuilt.extend(&mut slow);
        assert_eq!(
            read_message(rebuilt.as_slice()).unwrap(),
            (STATUS, b"running".to_vec())
        );
    }

    #[test]
    fn key_round_trip() {
        assert_eq!(decode_key(&encode_key(27, 72)), Some((27, 72)));
    }

    #[test]
    fn rejects_oversized_payloads_before_allocation() {
        let mut header = vec![FRAME];
        header.extend_from_slice(&((MAX_PAYLOAD + 1) as u32).to_le_bytes());
        assert_eq!(
            read_message(header.as_slice()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            write_message(Vec::new(), FRAME, &vec![0; MAX_PAYLOAD + 1])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn rejects_short_key_payloads() {
        assert_eq!(decode_key_event(&[0; 15]), None);
    }

    #[test]
    fn mouse_round_trip() {
        assert_eq!(
            decode_mouse(&encode_mouse(150, 250, true, false)),
            Some((150, 250, true, false))
        );
        assert_eq!(decode_mouse(&[0; 16]), None);
    }
}
