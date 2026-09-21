//! The savegame container as the game writes it.
//!
//! A file is a 53-byte header followed by a payload: the magic `SSF1`,
//! the payload's length, the length of the JSON below it, the payload's
//! md5 as hex, and one byte naming the encoding. The game's own names
//! for that byte are `CONFIG_ENCRYPTION_UNENCRYPTED` (0), `deflate` (1)
//! and `deflateXorSkip` (2); retail writes 2 and nothing else.
//!
//! `deflateXorSkip` masks a plain zlib stream. The mask is its own
//! inverse, so encoding and decoding are the same walk: a control byte
//! passes through untouched and its value says how many of the bytes
//! after it are XORed with the rotating four-byte key; then the next
//! byte is a control byte again.
//!
//! The key is a constant of one game build. If an update changes it,
//! `decode` fails here and the feature above switches itself off —
//! which is the only acceptable outcome, because half-read save data
//! would be worse than none.

use crate::error::{Error, Result, SaveDataDefect};
use md5::{Digest, Md5};

/// The magic every savegame file starts with.
const MAGIC: &[u8; 4] = b"SSF1";
/// Magic, both lengths, the md5 hex and the encoding byte.
const HEADER_LEN: usize = 53;
/// The only encoding the retail game writes: `deflateXorSkip`.
const ENCODING_XOR_SKIP: u8 = 2;
/// Written by a static initialiser in the game's executable as
/// `movl $0xecaa649d`, so little-endian these four bytes.
const XOR_KEY: [u8; 4] = [0x9d, 0x64, 0xaa, 0xec];

/// Applies `deflateXorSkip`. Its own inverse — see the module comment.
fn mask(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut key_index = 0usize;
    let mut masked = 0u8;
    let mut run = 0u8;
    for &byte in data {
        if masked == run {
            run = byte;
            masked = 0;
            out.push(byte);
        } else {
            out.push(byte ^ XOR_KEY[key_index]);
            key_index = (key_index + 1) % XOR_KEY.len();
            masked += 1;
        }
    }
    out
}

fn hex_md5(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn deflate(data: &[u8]) -> Vec<u8> {
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("writing into a Vec cannot fail");
    encoder.finish().expect("finishing a Vec writer cannot fail")
}

fn inflate(stream: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let mut out = Vec::new();
    ZlibDecoder::new(stream)
        .read_to_end(&mut out)
        .map_err(|_| Error::UnreadableSaveData(SaveDataDefect::NotDeflate))?;
    Ok(out)
}

/// The JSON inside a savegame file.
///
/// Every field the header declares is checked before the result is
/// handed out: a file that disagrees with itself is rejected rather
/// than read as far as it goes.
pub fn decode(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() < HEADER_LEN {
        return Err(Error::UnreadableSaveData(SaveDataDefect::TooShort { len: bytes.len() }));
    }
    if &bytes[..4] != MAGIC {
        return Err(Error::UnreadableSaveData(SaveDataDefect::NotSsf1));
    }
    let declared_payload = u64::from_le_bytes(bytes[4..12].try_into().expect("eight bytes"));
    let declared_content = u64::from_le_bytes(bytes[12..20].try_into().expect("eight bytes"));
    let checksum = std::str::from_utf8(&bytes[20..52])
        .map_err(|_| Error::UnreadableSaveData(SaveDataDefect::ChecksumMismatch))?;
    let encoding = bytes[52];
    if encoding != ENCODING_XOR_SKIP {
        return Err(Error::UnreadableSaveData(SaveDataDefect::UnsupportedEncoding { encoding }));
    }

    let payload = &bytes[HEADER_LEN..];
    if payload.len() as u64 != declared_payload {
        return Err(Error::UnreadableSaveData(SaveDataDefect::PayloadLengthMismatch {
            declared: declared_payload,
            actual: payload.len() as u64,
        }));
    }
    if hex_md5(payload) != checksum {
        return Err(Error::UnreadableSaveData(SaveDataDefect::ChecksumMismatch));
    }

    let content = inflate(&mask(payload))?;
    if content.len() as u64 != declared_content {
        return Err(Error::UnreadableSaveData(SaveDataDefect::ContentLengthMismatch {
            declared: declared_content,
            actual: content.len() as u64,
        }));
    }
    Ok(content)
}

/// A savegame file holding `json`, in the shape the game reads.
pub fn encode(json: &[u8]) -> Vec<u8> {
    let payload = mask(&deflate(json));
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&(json.len() as u64).to_le_bytes());
    out.extend_from_slice(hex_md5(&payload).as_bytes());
    out.push(ENCODING_XOR_SKIP);
    out.extend_from_slice(&payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT: &[u8] = br#"{"Economy":{"systemVersion":700,"States":{}}}"#;

    #[test]
    fn a_document_survives_encoding_and_decoding() {
        let file = encode(DOCUMENT);
        assert_eq!(&file[..4], b"SSF1");
        assert_eq!(decode(&file).unwrap(), DOCUMENT);
    }

    #[test]
    fn the_payload_starts_with_the_masked_zlib_header() {
        // Every file the game writes starts `78 01`: the `78` is the
        // zlib CMF byte passing through as a control byte, the `01` is
        // `9c ^ 9d`. Getting this wrong means the mask is misaligned.
        let file = encode(DOCUMENT);
        assert_eq!(&file[HEADER_LEN..HEADER_LEN + 2], &[0x78, 0x01]);
    }

    #[test]
    fn a_flipped_payload_byte_is_caught_by_the_checksum() {
        let mut file = encode(DOCUMENT);
        let last = file.len() - 1;
        file[last] ^= 0xff;
        assert!(matches!(
            decode(&file),
            Err(Error::UnreadableSaveData(SaveDataDefect::ChecksumMismatch))
        ));
    }

    #[test]
    fn a_foreign_magic_is_rejected() {
        let mut file = encode(DOCUMENT);
        file[..4].copy_from_slice(b"SSF2");
        assert!(matches!(
            decode(&file),
            Err(Error::UnreadableSaveData(SaveDataDefect::NotSsf1))
        ));
    }

    #[test]
    fn the_encodings_the_retail_game_never_writes_are_rejected() {
        for encoding in [0u8, 1] {
            let mut file = encode(DOCUMENT);
            file[52] = encoding;
            assert!(matches!(
                decode(&file),
                Err(Error::UnreadableSaveData(SaveDataDefect::UnsupportedEncoding { .. }))
            ));
        }
    }

    #[test]
    fn a_file_shorter_than_its_header_is_rejected() {
        assert!(matches!(
            decode(b"SSF1"),
            Err(Error::UnreadableSaveData(SaveDataDefect::TooShort { .. }))
        ));
    }

    #[test]
    fn a_truncated_payload_is_rejected_before_its_checksum() {
        let mut file = encode(DOCUMENT);
        file.pop();
        assert!(matches!(
            decode(&file),
            Err(Error::UnreadableSaveData(SaveDataDefect::PayloadLengthMismatch { .. }))
        ));
    }
}
