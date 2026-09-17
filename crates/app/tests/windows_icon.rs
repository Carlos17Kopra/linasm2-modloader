//! The icon `build.rs` builds into the Windows executable.
//!
//! Runs on every platform, and that is the point: the resource compiler
//! that consumes this file only exists on Windows, so without this check
//! a broken `.ico` would first be noticed by whoever downloads the
//! release. The format is simple enough to read without a decoder — a
//! six byte header, then sixteen bytes per image — and reading it here
//! costs nothing.

use std::path::PathBuf;

/// The sizes `packaging/make-icons.py` writes into the file: 16 and 32
/// for lists, 48 for the desktop, 256 for Explorer's large view.
const EXPECTED_SIZES: [u32; 4] = [16, 32, 48, 256];

fn icon_bytes() -> Vec<u8> {
    let path: PathBuf =
        [env!("CARGO_MANIFEST_DIR"), "..", "..", "packaging", "lina-sm2.ico"].iter().collect();
    std::fs::read(&path).unwrap_or_else(|e| panic!("{} is not readable: {e}", path.display()))
}

/// The sizes held in the file, in the order its directory lists them. A
/// width or height byte of 0 means 256 in this format, which is the
/// reason the largest size cannot simply be read as a number.
///
/// Every entry's image data is checked to lie inside the file as well.
/// Reading only the directory would let a file truncated anywhere after
/// its first 70 bytes pass as sound — which is exactly what an earlier
/// version of this test did when it was held against a deliberately
/// shortened copy.
fn sizes_in(bytes: &[u8]) -> Vec<(u32, u32)> {
    assert!(bytes.len() >= 6, "shorter than an icon directory header");
    assert_eq!(&bytes[0..2], &[0, 0], "reserved field is not zero — not an .ico");
    assert_eq!(&bytes[2..4], &[1, 0], "type field does not say icon");

    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    assert!(count > 0, "the file holds no images at all");
    assert!(bytes.len() >= 6 + count * 16, "the directory is cut short");

    (0..count)
        .map(|i| {
            let entry = 6 + i * 16;
            let word = |at: usize| {
                u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
            };
            let length = word(entry + 8);
            let offset = word(entry + 12);
            assert!(
                offset.saturating_add(length) <= bytes.len(),
                "image {i} claims {length} bytes at {offset}, past the end of a {} byte file",
                bytes.len()
            );

            let read = |b: u8| if b == 0 { 256 } else { u32::from(b) };
            (read(bytes[entry]), read(bytes[entry + 1]))
        })
        .collect()
}

#[test]
fn the_windows_icon_holds_every_size_windows_asks_for() {
    let sizes = sizes_in(&icon_bytes());

    let mut widths: Vec<u32> = sizes.iter().map(|(w, _)| *w).collect();
    widths.sort_unstable();
    assert_eq!(
        widths,
        EXPECTED_SIZES.to_vec(),
        "run packaging/make-icons.py — the sizes in the .ico have drifted"
    );
}

#[test]
fn every_image_in_the_windows_icon_is_square() {
    for (width, height) in sizes_in(&icon_bytes()) {
        assert_eq!(width, height, "a {width}x{height} image would be shown stretched");
    }
}
