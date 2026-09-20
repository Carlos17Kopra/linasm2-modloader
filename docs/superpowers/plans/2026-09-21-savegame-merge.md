# Savegame Merge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Compose a new savegame backup out of several existing ones —
one backup as the base, individual parts taken from others — usable from
the command line, with no interface work.

**Architecture:** A new `savedata` module in `sm2-core` holds three
layers that know nothing of each other: `ssf1` turns a savegame file into
JSON and back, `catalogue` says what a part is and where it sits, and
`compose` merges documents. Writing the result goes through the existing
backup machinery in `saves.rs`, so archive, manifest and crash safety are
untouched. The live save directory is never written.

**Tech Stack:** Rust 2024, `flate2` (pure-Rust zlib), `md-5`,
`serde_json` with `preserve_order`, `zip`, `clap`.

**Spec:** `docs/superpowers/specs/2026-09-21-savegame-merge-design.md`

## Global Constraints

- MSRV: `sm2-core` 1.85, `lina-sm2` 1.95. Nothing newer.
- Code, comments and test names are English. Comment prose wraps at 78
  columns including the `///` prefix.
- Every sentence a user sees lives in `crates/core/i18n/en.toml` and
  `de.toml`, reached with `t!("area.key")`. Both files carry exactly the
  same keys. German literals in the sources fail
  `crates/app/tests/no_german_literals.rs`.
- Every new clap argument needs a `cli.<path>.arg.<name>` key, or
  `every_command_and_argument_has_a_key` fails.
- Any test whose assertion depends on the active language holds
  `sm2_core::i18n::language_test_lock()` (in `crates/core`) or
  `crate::app_state::language_test_lock()` (in `crates/app`).
- `cli::requires_exclusive_access` is matched exhaustively with no `_`
  arm; a new subcommand does not compile until it has a side.
- Tests first. `cargo test` and `cargo clippy --all-targets` stay green.
- Nothing outside the backup directory is ever written.

---

### Task 1: The SSF1 container

Turns a savegame file into its JSON and back. This is the only place
that knows the format.

**Files:**
- Create: `crates/core/src/savedata/mod.rs`
- Create: `crates/core/src/savedata/ssf1.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod savedata;`)
- Modify: `crates/core/src/error.rs` (add `SaveData(SaveDataDefect)`)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`
- Modify: `Cargo.toml`, `crates/core/Cargo.toml`
- Test: in-file `#[cfg(test)] mod tests` in `ssf1.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `sm2_core::savedata::ssf1::decode(bytes: &[u8]) -> Result<Vec<u8>>`
  - `sm2_core::savedata::ssf1::encode(json: &[u8]) -> Vec<u8>`
  - `sm2_core::error::SaveDataDefect` (enum, variants below)

- [ ] **Step 1: Add the dependencies**

In the workspace `Cargo.toml`, under `[workspace.dependencies]`:

```toml
flate2 = "1.1"
md-5 = "0.11"
```

And change the existing `serde_json` line to:

```toml
serde_json = { version = "1", features = ["preserve_order"] }
```

In `crates/core/Cargo.toml`, under `[dependencies]`:

```toml
flate2.workspace = true
md-5.workspace = true
```

`preserve_order` keeps a document's member order when it is written
again. It is a workspace-wide feature and reaches every other user of
`serde_json`; that is intended.

- [ ] **Step 2: Run the build to confirm the dependencies resolve**

Run: `cargo build -p sm2-core`
Expected: PASS, no code change yet.

- [ ] **Step 3: Add the error variants**

In `crates/core/src/error.rs`, add to `enum Error` next to
`CorruptBackup(BackupDefect)`:

```rust
    /// A savegame file that cannot be read as the game wrote it.
    UnreadableSaveData(SaveDataDefect),
```

Add the defect enum next to `BackupDefect`:

```rust
/// Why a savegame file cannot be read — same idea as `BackupDefect`.
#[derive(Debug)]
pub enum SaveDataDefect {
    TooShort { len: usize },
    NotSsf1,
    UnsupportedEncoding { encoding: u8 },
    PayloadLengthMismatch { declared: u64, actual: u64 },
    ContentLengthMismatch { declared: u64, actual: u64 },
    ChecksumMismatch,
    NotDeflate,
    NotJson,
}

impl SaveDataDefect {
    fn text(&self) -> String {
        match self {
            SaveDataDefect::TooShort { len } => {
                i18n::format("error.save_data_defect.too_short", &[("len", len.to_string())])
            }
            SaveDataDefect::NotSsf1 => i18n::lookup("error.save_data_defect.not_ssf1"),
            SaveDataDefect::UnsupportedEncoding { encoding } => i18n::format(
                "error.save_data_defect.unsupported_encoding",
                &[("encoding", encoding.to_string())],
            ),
            SaveDataDefect::PayloadLengthMismatch { declared, actual } => i18n::format(
                "error.save_data_defect.payload_length_mismatch",
                &[("declared", declared.to_string()), ("actual", actual.to_string())],
            ),
            SaveDataDefect::ContentLengthMismatch { declared, actual } => i18n::format(
                "error.save_data_defect.content_length_mismatch",
                &[("declared", declared.to_string()), ("actual", actual.to_string())],
            ),
            SaveDataDefect::ChecksumMismatch => {
                i18n::lookup("error.save_data_defect.checksum_mismatch")
            }
            SaveDataDefect::NotDeflate => i18n::lookup("error.save_data_defect.not_deflate"),
            SaveDataDefect::NotJson => i18n::lookup("error.save_data_defect.not_json"),
        }
    }
}
```

And in `impl std::fmt::Display for Error`, next to the `CorruptBackup`
arm:

```rust
            Error::UnreadableSaveData(defect) => defect.text(),
```

- [ ] **Step 4: Add the message keys**

In `crates/core/i18n/en.toml`, a new section after
`[error.backup_defect]`:

```toml
[error.save_data_defect]
too_short = "The savegame file is too short to be one: {len} bytes."
not_ssf1 = "This is not a savegame file the game wrote."
unsupported_encoding = "The savegame file uses encoding {encoding}, which this version cannot read."
payload_length_mismatch = "The savegame file says it holds {declared} bytes and holds {actual}."
content_length_mismatch = "The savegame file says it unpacks to {declared} bytes and unpacks to {actual}."
checksum_mismatch = "The savegame file does not match its own checksum."
not_deflate = "The savegame file cannot be unpacked. A game update may have changed the format."
not_json = "The savegame file does not hold the data this version expects."
```

In `crates/core/i18n/de.toml`, the same keys:

```toml
[error.save_data_defect]
too_short = "Die Spielstandsdatei ist zu kurz, um eine zu sein: {len} Bytes."
not_ssf1 = "Das ist keine Spielstandsdatei des Spiels."
unsupported_encoding = "Die Spielstandsdatei nutzt Kodierung {encoding}, die diese Version nicht lesen kann."
payload_length_mismatch = "Die Spielstandsdatei nennt {declared} Bytes und enthält {actual}."
content_length_mismatch = "Die Spielstandsdatei nennt {declared} Bytes nach dem Entpacken und liefert {actual}."
checksum_mismatch = "Die Spielstandsdatei passt nicht zu ihrer eigenen Prüfsumme."
not_deflate = "Die Spielstandsdatei lässt sich nicht entpacken. Ein Spiel-Update kann das Format geändert haben."
not_json = "Die Spielstandsdatei enthält nicht die Daten, die diese Version erwartet."
```

- [ ] **Step 5: Write the failing tests**

Create `crates/core/src/savedata/mod.rs`:

```rust
//! Reading the game's savegame files and composing new ones out of
//! several backups.

pub mod ssf1;
```

Create `crates/core/src/savedata/ssf1.rs` with only the tests for now:

```rust
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
```

Add `pub mod savedata;` to `crates/core/src/lib.rs`, in alphabetical
order between `pub mod profile;` and `pub mod saves;`.

- [ ] **Step 6: Run the tests to verify they fail**

Run: `cargo test -p sm2-core savedata`
Expected: FAIL — `cannot find function encode in this scope`.

- [ ] **Step 7: Write the implementation**

Above the test module in `crates/core/src/savedata/ssf1.rs`:

```rust
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
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p sm2-core savedata`
Expected: PASS, seven tests.

- [ ] **Step 9: Run the whole suite and clippy**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS. `every_language_has_exactly_the_english_keys` proves the
two catalogues agree.

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml crates/core/Cargo.toml crates/core/src/lib.rs \
  crates/core/src/error.rs crates/core/src/savedata crates/core/i18n
git commit -m "feat(savedata): read and write the game's savegame files"
```

---

### Task 2: Reading a backup's files

`compose` needs the contents of a backup without unpacking it into the
save directory. `restore` cannot be used for that — it writes.

**Files:**
- Modify: `crates/core/src/saves.rs` (new `read_files`, near `verify`)
- Test: in-file `mod tests` in `saves.rs`

**Interfaces:**
- Consumes: `BackupEntry`, `validate_entry_name` (both already there).
- Produces:
  - `sm2_core::saves::read_files(entry: &BackupEntry) -> Result<BTreeMap<String, Vec<u8>>>`

- [ ] **Step 1: Write the failing test**

In the existing `mod tests` of `crates/core/src/saves.rs`:

```rust
    #[test]
    fn read_files_returns_every_file_of_a_backup_by_its_relative_path() {
        let (_temp, save_dir, backup_root) = save_fixture();
        std::fs::create_dir_all(save_dir.join("config")).unwrap();
        std::fs::write(save_dir.join("config/economy.cfg"), b"payload").unwrap();

        let entry = backup(&save_dir, &backup_root, None).unwrap();
        let files = read_files(&entry).unwrap();

        assert_eq!(files.get("config/economy.cfg").map(Vec::as_slice), Some(&b"payload"[..]));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p sm2-core read_files_returns_every_file`
Expected: FAIL — `cannot find function read_files`.

- [ ] **Step 3: Write the implementation**

In `crates/core/src/saves.rs`, directly after `verify`:

```rust
/// Every file of a backup, keyed by its path relative to the save
/// directory.
///
/// This reads the archive without touching the save directory, which is
/// what composing needs: it looks into several backups at once and
/// writes a new one, never into the game's own directory. The caller
/// runs `verify` first — this function only guards the entry names,
/// because a name that leaves the save directory must not even become a
/// map key.
pub fn read_files(entry: &BackupEntry) -> Result<BTreeMap<String, Vec<u8>>> {
    let file = std::fs::File::open(&entry.archive).map_err(|e| Error::io(&entry.archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| {
        Error::CorruptBackup(BackupDefect::NotAZip { path: entry.archive.clone() })
    })?;

    let mut files = BTreeMap::new();
    for i in 0..zip.len() {
        let mut zip_entry = zip.by_index(i).map_err(|e| {
            Error::io(&entry.archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        if !zip_entry.is_file() {
            continue;
        }
        let name = zip_entry.name().to_string();
        validate_entry_name(&name)?;
        let mut content = Vec::new();
        std::io::copy(&mut zip_entry, &mut content).map_err(|e| Error::io(&entry.archive, e))?;
        if files.insert(name.clone(), content).is_some() {
            return Err(Error::CorruptBackup(BackupDefect::DuplicateEntry { name }));
        }
    }
    Ok(files)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p sm2-core read_files_returns_every_file`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/saves.rs
git commit -m "feat(saves): read a backup's files without restoring it"
```

---

### Task 3: The part catalogue

What a part is, where it sits, and which parts a given backup offers.
The groups are a static table; the ids below them are read out of the
data, so a game update that adds a weapon needs no code change.

**Files:**
- Create: `crates/core/src/savedata/catalogue.rs`
- Modify: `crates/core/src/savedata/mod.rs`
- Test: in-file `mod tests` in `catalogue.rs`

**Interfaces:**
- Consumes: `ssf1::decode`.
- Produces:
  - `pub type Documents = std::collections::BTreeMap<String, serde_json::Value>`
  - `pub fn documents(files: &BTreeMap<String, Vec<u8>>) -> Result<Documents>`
  - `pub struct Part { pub id: String, pub group: &'static str, pub file: &'static str, pub selector: Selector }`
  - `pub enum Selector { Whole, Member { container: &'static str, key: String }, ListItem { container: &'static str, key_field: &'static str, id: String } }`
  - `impl Selector { pub fn get<'a>(&self, document: &'a Value) -> Option<&'a Value>; pub fn set(&self, document: &mut Value, value: Value) -> bool }`
  - `pub fn parts(documents: &Documents) -> Vec<Part>`
  - `pub fn part_by_id(documents: &Documents, id: &str) -> Option<Part>`
  - `pub const GROUPS: &[Group]`

- [ ] **Step 1: Write the failing tests**

Create `crates/core/src/savedata/catalogue.rs` with the tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn documents_fixture() -> Documents {
        let mut documents = Documents::new();
        documents.insert(
            "config/user_progression.cfg".to_string(),
            json!({"UserProgression": {"systemVersion": 767, "UserMastery": {
                "masteryStates": {
                    "PVE_TANK": {"json_version": 3, "currentLevel": 5},
                    "PVE_SOLDIER": {"json_version": 3, "currentLevel": 19}
                }}}}),
        );
        documents.insert(
            "config/loadouts.cfg".to_string(),
            json!({"Loadouts": {"systemVersion": 1277, "Sets": {"loadoutSets": [
                {"json_version": 2, "masteryUid": "STORY_TITUS"},
                {"json_version": 2, "masteryUid": "STORY_GADRIEL"}
            ]}}}),
        );
        documents.insert(
            "config/economy.cfg".to_string(),
            json!({"Economy": {"systemVersion": 700, "States": {}}}),
        );
        documents
    }

    #[test]
    fn the_catalogue_offers_one_part_per_id_found_in_the_data() {
        let parts = parts(&documents_fixture());
        let ids: Vec<&str> = parts.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"class_level:PVE_TANK"));
        assert!(ids.contains(&"class_level:PVE_SOLDIER"));
        assert!(ids.contains(&"loadout:STORY_TITUS"));
        assert!(ids.contains(&"economy"));
    }

    #[test]
    fn a_file_that_is_absent_offers_no_parts_instead_of_failing() {
        let mut documents = documents_fixture();
        documents.remove("config/user_progression.cfg");
        let parts = parts(&documents);
        assert!(!parts.iter().any(|p| p.group == "class_level"));
        assert!(parts.iter().any(|p| p.id == "economy"));
    }

    #[test]
    fn no_part_contains_another() {
        // The rule the whole feature rests on: two selections can never
        // contradict each other, because no two parts overlap. Parts of
        // the same file must therefore differ in their selector, and a
        // whole-file part must be the only part of its file.
        let documents = documents_fixture();
        let parts = parts(&documents);
        for (i, a) in parts.iter().enumerate() {
            for b in parts.iter().skip(i + 1) {
                if a.file != b.file {
                    continue;
                }
                assert!(
                    !matches!(a.selector, Selector::Whole) && !matches!(b.selector, Selector::Whole),
                    "{} and {} share a file and one takes all of it",
                    a.id,
                    b.id
                );
                assert_ne!(a.selector, b.selector, "{} and {} select the same node", a.id, b.id);
            }
        }
    }

    #[test]
    fn a_member_selector_reads_and_replaces_exactly_its_node() {
        let documents = documents_fixture();
        let part = part_by_id(&documents, "class_level:PVE_TANK").unwrap();
        let mut document = documents["config/user_progression.cfg"].clone();

        assert_eq!(part.selector.get(&document).unwrap()["currentLevel"], json!(5));
        assert!(part.selector.set(&mut document, json!({"json_version": 3, "currentLevel": 42})));

        let states = &document["UserProgression"]["UserMastery"]["masteryStates"];
        assert_eq!(states["PVE_TANK"]["currentLevel"], json!(42));
        assert_eq!(states["PVE_SOLDIER"]["currentLevel"], json!(19), "the neighbour moved");
    }

    #[test]
    fn a_list_selector_finds_its_element_by_key_not_by_position() {
        let documents = documents_fixture();
        let part = part_by_id(&documents, "loadout:STORY_GADRIEL").unwrap();
        // The same set, but stored first instead of second.
        let mut document = json!({"Loadouts": {"systemVersion": 1, "Sets": {"loadoutSets": [
            {"json_version": 2, "masteryUid": "STORY_GADRIEL"},
            {"json_version": 2, "masteryUid": "STORY_TITUS"}
        ]}}});
        assert!(part.selector.set(&mut document, json!({"json_version": 2, "masteryUid": "STORY_GADRIEL", "marked": true})));

        let sets = document["Loadouts"]["Sets"]["loadoutSets"].as_array().unwrap();
        assert_eq!(sets[0]["marked"], json!(true));
        assert_eq!(sets[1]["masteryUid"], json!("STORY_TITUS"), "the neighbour moved");
    }

    #[test]
    fn a_whole_file_selector_reads_and_replaces_the_system_object() {
        let documents = documents_fixture();
        let part = part_by_id(&documents, "economy").unwrap();
        let mut document = documents["config/economy.cfg"].clone();

        assert_eq!(part.selector.get(&document).unwrap()["systemVersion"], json!(700));
        assert!(part.selector.set(&mut document, json!({"systemVersion": 701, "States": {}})));
        assert_eq!(document["Economy"]["systemVersion"], json!(701));
    }

    #[test]
    fn a_selector_whose_node_is_absent_reports_it_rather_than_inventing_one() {
        let documents = documents_fixture();
        let part = part_by_id(&documents, "class_level:PVE_TANK").unwrap();
        let mut document = json!({"UserProgression": {"UserMastery": {"masteryStates": {}}}});
        assert!(part.selector.get(&document).is_none());
        assert!(!part.selector.set(&mut document, json!({})));
    }
}
```

Add to `crates/core/src/savedata/mod.rs`:

```rust
pub mod catalogue;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sm2-core catalogue`
Expected: FAIL — `cannot find type Documents`.

- [ ] **Step 3: Write the implementation**

Above the tests in `crates/core/src/savedata/catalogue.rs`:

```rust
//! What a part is, and which parts a backup offers.
//!
//! The groups below are a static table: which file a kind of data lives
//! in, where inside it, and what identifies one entry. The ids
//! themselves — 45 weapons, 29 classes — are read out of the backup
//! being looked at, so a game update that adds one needs no code change
//! here, and a backup from an older build simply offers fewer parts.
//!
//! The one rule this table has to keep is that no part contains
//! another. A file either has a whole-file part or finer ones, never
//! both, and two finer parts never select the same node. Without that
//! rule two selections could contradict each other and the result would
//! depend on the order they were applied in.

use crate::error::{Error, Result, SaveDataDefect};
use crate::savedata::ssf1;
use serde_json::Value;
use std::collections::BTreeMap;

/// The decoded savegame files of one backup, keyed by the path relative
/// to the save directory.
pub type Documents = BTreeMap<String, Value>;

/// Where a part's data sits inside one file.
#[derive(Debug, Clone, PartialEq)]
pub enum Selector {
    /// The file's single system object — everything below the root.
    Whole,
    /// A member of the object at `container`, by its key.
    Member { container: &'static str, key: String },
    /// The element of the array at `container` whose `key_field` is `id`.
    ListItem { container: &'static str, key_field: &'static str, id: String },
}

/// How the parts of one file are found.
#[derive(Debug, Clone, Copy)]
pub enum GroupKind {
    /// The file is one part.
    WholeFile,
    /// Every member of the object at this pointer is a part.
    Members { container: &'static str },
    /// Every element of the array at this pointer is a part, identified
    /// by the named field.
    ListItems { container: &'static str, key_field: &'static str },
}

/// A kind of data the user can take from another backup.
#[derive(Debug, Clone, Copy)]
pub struct Group {
    pub id: &'static str,
    pub file: &'static str,
    pub kind: GroupKind,
}

/// One selectable piece of a backup.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub id: String,
    pub group: &'static str,
    pub file: &'static str,
    pub selector: Selector,
}

/// Everything a composition can move between backups.
///
/// Settings, agreements, achievements bookkeeping and the platform's
/// own record are deliberately absent: they are not progression, and
/// mixing them between saves buys nothing.
pub const GROUPS: &[Group] = &[
    Group {
        id: "class_level",
        file: "config/user_progression.cfg",
        kind: GroupKind::Members { container: "/UserProgression/UserMastery/masteryStates" },
    },
    Group {
        id: "armour_loyalist",
        file: "config/character_customization_progression.cfg",
        kind: GroupKind::Members {
            container: "/CharacterCustomizationProgression/CharacterCustomization/teamStates/LOYALIST/outfitStates",
        },
    },
    Group {
        id: "armour_chaos",
        file: "config/character_customization_progression.cfg",
        kind: GroupKind::Members {
            container: "/CharacterCustomizationProgression/CharacterCustomization/teamStates/CHAOS/outfitStates",
        },
    },
    Group {
        id: "weapon",
        file: "config/weapon_progression.cfg",
        kind: GroupKind::Members { container: "/WeaponProgression/WeaponMastery/weaponStates" },
    },
    Group {
        id: "heraldry",
        file: "config/heraldry_progression.cfg",
        kind: GroupKind::Members { container: "/HeraldryProgression/Armor/characterArmorInfos" },
    },
    Group {
        id: "loadout",
        file: "config/loadouts.cfg",
        kind: GroupKind::ListItems {
            container: "/Loadouts/Sets/loadoutSets",
            key_field: "masteryUid",
        },
    },
    Group { id: "challenges", file: "config/challenge_progression.cfg", kind: GroupKind::WholeFile },
    Group { id: "story", file: "config/story_progression.cfg", kind: GroupKind::WholeFile },
    Group { id: "economy", file: "config/economy.cfg", kind: GroupKind::WholeFile },
    Group { id: "pve_state", file: "config/pve_state.cfg", kind: GroupKind::WholeFile },
    Group { id: "horde_mode", file: "config/hordemode_state.cfg", kind: GroupKind::WholeFile },
    Group { id: "tutorial", file: "config/tutorial.cfg", kind: GroupKind::WholeFile },
    Group { id: "mutators", file: "config/mutator_challenges.cfg", kind: GroupKind::WholeFile },
    Group { id: "achievements", file: "config/achievements.cfg", kind: GroupKind::WholeFile },
];

/// The key of a document's single system object, e.g. `UserProgression`.
pub fn system_key(document: &Value) -> Option<&str> {
    let object = document.as_object()?;
    if object.len() != 1 {
        return None;
    }
    object.keys().next().map(String::as_str)
}

impl Selector {
    /// The value this selector points at, or `None` when it is absent —
    /// which is the normal answer for a backup from an older build, not
    /// an error.
    pub fn get<'a>(&self, document: &'a Value) -> Option<&'a Value> {
        match self {
            Selector::Whole => document.get(system_key(document)?),
            Selector::Member { container, key } => document.pointer(container)?.get(key),
            Selector::ListItem { container, key_field, id } => document
                .pointer(container)?
                .as_array()?
                .iter()
                .find(|element| element.get(key_field).and_then(Value::as_str) == Some(id)),
        }
    }

    /// Replaces that value. `false` means the target is not there and
    /// nothing was written — no node is ever created.
    pub fn set(&self, document: &mut Value, value: Value) -> bool {
        match self {
            Selector::Whole => {
                let Some(key) = system_key(document).map(str::to_string) else {
                    return false;
                };
                match document.get_mut(&key) {
                    Some(slot) => {
                        *slot = value;
                        true
                    }
                    None => false,
                }
            }
            Selector::Member { container, key } => {
                match document.pointer_mut(container).and_then(|node| node.get_mut(key)) {
                    Some(slot) => {
                        *slot = value;
                        true
                    }
                    None => false,
                }
            }
            Selector::ListItem { container, key_field, id } => {
                let Some(array) = document.pointer_mut(container).and_then(Value::as_array_mut)
                else {
                    return false;
                };
                match array
                    .iter_mut()
                    .find(|element| element.get(key_field).and_then(Value::as_str) == Some(id))
                {
                    Some(slot) => {
                        *slot = value;
                        true
                    }
                    None => false,
                }
            }
        }
    }
}

/// Decodes every savegame file of a backup that the catalogue knows.
///
/// Files outside the catalogue are left as bytes: a composition copies
/// them through untouched and never needs to understand them.
pub fn documents(files: &BTreeMap<String, Vec<u8>>) -> Result<Documents> {
    let mut documents = Documents::new();
    for group in GROUPS {
        if documents.contains_key(group.file) {
            continue;
        }
        let Some(bytes) = files.get(group.file) else {
            continue;
        };
        let json = ssf1::decode(bytes)?;
        let value: Value = serde_json::from_slice(&json)
            .map_err(|_| Error::UnreadableSaveData(SaveDataDefect::NotJson))?;
        documents.insert(group.file.to_string(), value);
    }
    Ok(documents)
}

/// Every part these documents offer, in the order of `GROUPS`.
pub fn parts(documents: &Documents) -> Vec<Part> {
    let mut parts = Vec::new();
    for group in GROUPS {
        let Some(document) = documents.get(group.file) else {
            continue;
        };
        match group.kind {
            GroupKind::WholeFile => parts.push(Part {
                id: group.id.to_string(),
                group: group.id,
                file: group.file,
                selector: Selector::Whole,
            }),
            GroupKind::Members { container } => {
                let Some(object) = document.pointer(container).and_then(Value::as_object) else {
                    continue;
                };
                for key in object.keys() {
                    parts.push(Part {
                        id: format!("{}:{key}", group.id),
                        group: group.id,
                        file: group.file,
                        selector: Selector::Member { container, key: key.clone() },
                    });
                }
            }
            GroupKind::ListItems { container, key_field } => {
                let Some(array) = document.pointer(container).and_then(Value::as_array) else {
                    continue;
                };
                for element in array {
                    let Some(id) = element.get(key_field).and_then(Value::as_str) else {
                        continue;
                    };
                    parts.push(Part {
                        id: format!("{}:{id}", group.id),
                        group: group.id,
                        file: group.file,
                        selector: Selector::ListItem {
                            container,
                            key_field,
                            id: id.to_string(),
                        },
                    });
                }
            }
        }
    }
    parts
}

/// The part with this id, if these documents offer it.
pub fn part_by_id(documents: &Documents, id: &str) -> Option<Part> {
    parts(documents).into_iter().find(|part| part.id == id)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sm2-core catalogue`
Expected: PASS, seven tests.

- [ ] **Step 5: Run the whole suite and clippy**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/savedata
git commit -m "feat(savedata): the catalogue of composable parts"
```

---

### Task 4: Merging documents

The merge itself, on decoded documents and nothing else: no files, no
archives. Everything that can refuse happens here.

**Files:**
- Create: `crates/core/src/savedata/merge.rs`
- Modify: `crates/core/src/savedata/mod.rs`
- Modify: `crates/core/src/error.rs` (two variants)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`
- Test: in-file `mod tests` in `merge.rs`

**Interfaces:**
- Consumes: `catalogue::{Documents, Part, Selector, system_key}`.
- Produces:
  - `pub fn apply(base: &mut Documents, part: &Part, source: &Documents) -> Result<()>`
  - `pub fn raise_system_version(base: &mut Documents, file: &str, source: &Documents)`

- [ ] **Step 1: Write the failing tests**

Create `crates/core/src/savedata/merge.rs` with the tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::savedata::catalogue::{part_by_id, Documents};
    use serde_json::json;

    fn progression(version: u64, level: u64, schema: u64) -> Documents {
        let mut documents = Documents::new();
        documents.insert(
            "config/user_progression.cfg".to_string(),
            json!({"UserProgression": {"systemVersion": version, "UserMastery": {
                "masteryStates": {
                    "PVE_TANK": {"json_version": schema, "currentLevel": level},
                    "PVE_SOLDIER": {"json_version": schema, "currentLevel": 1}
                }}}}),
        );
        documents
    }

    #[test]
    fn a_part_is_taken_from_the_source_and_nothing_else_is() {
        let mut base = progression(700, 5, 3);
        let source = progression(701, 42, 3);
        let part = part_by_id(&base, "class_level:PVE_TANK").unwrap();

        apply(&mut base, &part, &source).unwrap();

        let states = &base["config/user_progression.cfg"]["UserProgression"]["UserMastery"]["masteryStates"];
        assert_eq!(states["PVE_TANK"]["currentLevel"], json!(42));
        assert_eq!(states["PVE_SOLDIER"]["currentLevel"], json!(1), "the neighbour moved");
    }

    #[test]
    fn a_schema_mismatch_is_refused_instead_of_merged() {
        let mut base = progression(700, 5, 3);
        let source = progression(701, 42, 4);
        let part = part_by_id(&base, "class_level:PVE_TANK").unwrap();

        let error = apply(&mut base, &part, &source).unwrap_err();
        assert!(matches!(error, Error::UnmergeablePart { .. }), "got {error:?}");

        let states = &base["config/user_progression.cfg"]["UserProgression"]["UserMastery"]["masteryStates"];
        assert_eq!(states["PVE_TANK"]["currentLevel"], json!(5), "base was changed anyway");
    }

    #[test]
    fn a_part_the_source_does_not_hold_is_refused() {
        let mut base = progression(700, 5, 3);
        let mut source = progression(701, 42, 3);
        source.remove("config/user_progression.cfg");
        let part = part_by_id(&base, "class_level:PVE_TANK").unwrap();

        assert!(matches!(
            apply(&mut base, &part, &source).unwrap_err(),
            Error::PartMissingInSource { .. }
        ));
    }

    #[test]
    fn the_system_version_becomes_the_highest_of_the_two() {
        let mut base = progression(700, 5, 3);
        let source = progression(890, 42, 3);
        raise_system_version(&mut base, "config/user_progression.cfg", &source);
        assert_eq!(base["config/user_progression.cfg"]["UserProgression"]["systemVersion"], json!(890));
    }

    #[test]
    fn a_lower_system_version_in_the_source_leaves_the_base_alone() {
        let mut base = progression(900, 5, 3);
        let source = progression(700, 42, 3);
        raise_system_version(&mut base, "config/user_progression.cfg", &source);
        assert_eq!(base["config/user_progression.cfg"]["UserProgression"]["systemVersion"], json!(900));
    }
}
```

Add to `crates/core/src/savedata/mod.rs`:

```rust
pub mod merge;
```

- [ ] **Step 2: Add the error variants and their keys**

In `crates/core/src/error.rs`, add to `enum Error`:

```rust
    /// A part whose schema version differs between the two backups.
    UnmergeablePart { part: String, base: String, source: String },
    /// A part the backup it should come from does not hold.
    PartMissingInSource { part: String },
```

And in `impl std::fmt::Display for Error`:

```rust
            Error::UnmergeablePart { part, base, source } => i18n::format(
                "error.unmergeable_part",
                &[("part", part.clone()), ("base", base.clone()), ("source", source.clone())],
            ),
            Error::PartMissingInSource { part } => {
                i18n::format("error.part_missing_in_source", &[("part", part.clone())])
            }
```

In `crates/core/i18n/en.toml`, under `[error]`:

```toml
unmergeable_part = "'{part}' cannot be taken over: the base holds schema version {base}, the source {source}. A game update has changed this data."
part_missing_in_source = "The backup chosen for '{part}' does not hold that part."
```

In `crates/core/i18n/de.toml`, under `[error]`:

```toml
unmergeable_part = "'{part}' lässt sich nicht übernehmen: die Basis führt Schemaversion {base}, die Quelle {source}. Ein Spiel-Update hat diese Daten geändert."
part_missing_in_source = "Das für '{part}' gewählte Backup enthält diesen Bestandteil nicht."
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p sm2-core merge`
Expected: FAIL — `cannot find function apply`.

- [ ] **Step 4: Write the implementation**

Above the tests in `crates/core/src/savedata/merge.rs`:

```rust
//! Merging one part of a savegame into another.
//!
//! Everything that can refuse lives here, and it refuses before it
//! writes: `apply` reads and checks the source first, so a rejected
//! part leaves the base exactly as it was. That matters because a
//! composition applies many parts in a row and the caller reports the
//! first refusal — a half-applied base would silently become the
//! result.

use crate::error::{Error, Result};
use crate::savedata::catalogue::{system_key, Documents, Part};
use serde_json::Value;

/// The schema version a node carries, if it carries one.
fn schema_version(node: &Value) -> Option<u64> {
    node.get("json_version")?.as_u64()
}

/// Takes one part out of `source` and puts it into `base`.
///
/// The schema versions have to match. `json_version` sits on the node
/// itself, so two backups from different game builds can disagree about
/// one part while agreeing about every other — which is why this is
/// decided per part and not per file.
pub fn apply(base: &mut Documents, part: &Part, source: &Documents) -> Result<()> {
    let source_document = source
        .get(part.file)
        .ok_or_else(|| Error::PartMissingInSource { part: part.id.clone() })?;
    let incoming = part
        .selector
        .get(source_document)
        .ok_or_else(|| Error::PartMissingInSource { part: part.id.clone() })?
        .clone();

    let base_document = base
        .get(part.file)
        .ok_or_else(|| Error::PartMissingInSource { part: part.id.clone() })?;
    let present = part
        .selector
        .get(base_document)
        .ok_or_else(|| Error::PartMissingInSource { part: part.id.clone() })?;

    if schema_version(present) != schema_version(&incoming) {
        return Err(Error::UnmergeablePart {
            part: part.id.clone(),
            base: schema_version(present).map_or_else(|| "-".to_string(), |v| v.to_string()),
            source: schema_version(&incoming).map_or_else(|| "-".to_string(), |v| v.to_string()),
        });
    }

    let base_document = base.get_mut(part.file).expect("checked above");
    if !part.selector.set(base_document, incoming) {
        return Err(Error::PartMissingInSource { part: part.id.clone() });
    }
    Ok(())
}

/// Lifts a file's `systemVersion` to the highest of base and source.
///
/// The counter is the engine's "how new is this" marker — it complains
/// that provided data is older than what it holds. A merged file is at
/// least as new as everything that went into it, so the highest value
/// wins. A file without the field is left alone.
pub fn raise_system_version(base: &mut Documents, file: &str, source: &Documents) {
    let Some(incoming) = source
        .get(file)
        .and_then(|document| {
            let key = system_key(document)?;
            document.get(key)?.get("systemVersion")?.as_u64()
        })
    else {
        return;
    };
    let Some(document) = base.get_mut(file) else {
        return;
    };
    let Some(key) = system_key(document).map(str::to_string) else {
        return;
    };
    let Some(slot) = document.get_mut(&key).and_then(|system| system.get_mut("systemVersion"))
    else {
        return;
    };
    if slot.as_u64().is_some_and(|present| present < incoming) {
        *slot = Value::from(incoming);
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p sm2-core merge`
Expected: PASS, five tests.

- [ ] **Step 6: Run the whole suite and clippy**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/savedata crates/core/src/error.rs crates/core/i18n
git commit -m "feat(savedata): merge a part of one savegame into another"
```

---

### Task 5: Recording a composition in the manifest

A composed backup says what it was made of. This changes a struct every
backup writes, so it comes before composing itself.

**Files:**
- Modify: `crates/core/src/saves.rs` (`BackupManifest`, `write_backup`,
  `backup_from`, `repair_layout`, `import_archive_limited`)
- Test: in-file `mod tests` in `saves.rs`

**Interfaces:**
- Consumes: `BackupManifest`.
- Produces:
  - `pub struct Composition { pub base: String, pub parts: BTreeMap<String, String> }`
  - `BackupManifest.composed_from: Option<Composition>`
  - `write_backup(..., composed_from: Option<Composition>)`

- [ ] **Step 1: Write the failing test**

In the existing `mod tests` of `crates/core/src/saves.rs`:

```rust
    #[test]
    fn an_ordinary_backup_records_no_composition() {
        let (_temp, save_dir, backup_root) = save_fixture();
        let entry = backup(&save_dir, &backup_root, None).unwrap();
        assert_eq!(read_manifest(&entry).unwrap().composed_from, None);
    }

    #[test]
    fn a_manifest_written_before_compositions_existed_still_reads() {
        let (_temp, save_dir, backup_root) = save_fixture();
        let entry = backup(&save_dir, &backup_root, None).unwrap();

        // Exactly the shape 0.5.1 wrote: no `composed_from` at all.
        let text = std::fs::read_to_string(&entry.manifest).unwrap();
        let without: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(without.get("composed_from").is_none(), "the field is written when unset");
        assert!(read_manifest(&entry).is_ok());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sm2-core composition`
Expected: FAIL — `no field composed_from on type BackupManifest`.

- [ ] **Step 3: Write the implementation**

In `crates/core/src/saves.rs`, next to `BackupManifest`:

```rust
/// What a composed backup was made of.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Composition {
    /// `created_at` of the backup that supplied everything not listed.
    pub base: String,
    /// Part id to the `created_at` of the backup it came from.
    pub parts: BTreeMap<String, String>,
}
```

Add the field to `BackupManifest`, after `label`:

```rust
    /// Set only on a backup that was composed out of others. Absent
    /// everywhere else, including in every manifest written before this
    /// field existed — hence `default` and `skip_serializing_if`, so an
    /// ordinary backup's manifest keeps the shape it has today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composed_from: Option<Composition>,
```

Give `write_backup` the parameter (it is private, so only its three
callers change):

```rust
fn write_backup(
    save_dir: &Path,
    backup_root: &Path,
    label: Option<&str>,
    source: String,
    created_at: String,
    composed_from: Option<Composition>,
) -> Result<BackupEntry> {
```

Set it where the manifest is built inside `write_backup`:

```rust
        composed_from,
```

`backup_from` passes `None`. In `repair_layout`, pass the existing
manifest's value through — a repaired backup is the same backup, and
losing its provenance would be a silent change:

```rust
    let composed_from = read_manifest(entry)?.composed_from;
```

and hand that to `write_backup`. `import_archive_limited` passes `None`:
an imported archive was never composed here.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sm2-core composition`
Expected: PASS.

- [ ] **Step 5: Run the whole suite and clippy**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS — in particular the existing repair and import tests.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/saves.rs
git commit -m "feat(saves): record in the manifest what a backup was composed of"
```

---

### Task 6: Composing a backup

The step that ties it together: read the backups, merge, write the
result as a new backup.

**Files:**
- Create: `crates/core/src/savedata/compose.rs`
- Modify: `crates/core/src/savedata/mod.rs`
- Modify: `crates/core/src/saves.rs` (`write_composition`)
- Test: in-file `mod tests` in `compose.rs`

**Interfaces:**
- Consumes: `saves::{read_files, verify, BackupEntry, Composition}`,
  `catalogue::{documents, part_by_id}`, `merge::{apply, raise_system_version}`,
  `ssf1::{decode, encode}`.
- Produces:
  - `pub fn compose(base: &BackupEntry, replacements: &[(String, BackupEntry)], backup_root: &Path, label: Option<&str>) -> Result<BackupEntry>`
  - `saves::write_composition(files: &BTreeMap<String, Vec<u8>>, backup_root: &Path, label: Option<&str>, source: &Path, composition: Composition) -> Result<BackupEntry>`
  - `saves::composition_of(entry: &BackupEntry) -> Result<Option<Composition>>`

- [ ] **Step 1: Write the failing tests**

Create `crates/core/src/savedata/compose.rs` with the tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::savedata::ssf1;

    /// A backup holding one progression file with `level` for the
    /// Bulwark, plus one file the catalogue knows nothing about.
    fn backup_fixture(
        root: &std::path::Path,
        label: &str,
        level: u64,
        stranger: &[u8],
    ) -> BackupEntry {
        let save_dir = root.join(format!("save-{label}"));
        std::fs::create_dir_all(save_dir.join("config")).unwrap();
        let json = format!(
            r#"{{"UserProgression":{{"systemVersion":{level},"UserMastery":{{"masteryStates":{{"PVE_TANK":{{"json_version":3,"currentLevel":{level}}}}}}}}}}}"#
        );
        std::fs::write(
            save_dir.join("config/user_progression.cfg"),
            ssf1::encode(json.as_bytes()),
        )
        .unwrap();
        std::fs::write(save_dir.join("config/unknown_to_us.cfg"), stranger).unwrap();
        crate::saves::backup(&save_dir, &root.join("backups"), Some(label)).unwrap()
    }

    fn level_of(entry: &BackupEntry) -> u64 {
        let files = crate::saves::read_files(entry).unwrap();
        let json = ssf1::decode(&files["config/user_progression.cfg"]).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        value["UserProgression"]["UserMastery"]["masteryStates"]["PVE_TANK"]["currentLevel"]
            .as_u64()
            .unwrap()
    }

    #[test]
    fn a_composition_without_replacements_reproduces_the_base_file_for_file() {
        let temp = tempfile::tempdir().unwrap();
        let base = backup_fixture(temp.path(), "base", 5, b"opaque");

        let composed =
            compose(&base, &[], &temp.path().join("backups"), Some("copy")).unwrap();

        assert_eq!(
            crate::saves::read_files(&base).unwrap(),
            crate::saves::read_files(&composed).unwrap()
        );
    }

    #[test]
    fn a_replaced_part_comes_from_the_source_backup() {
        let temp = tempfile::tempdir().unwrap();
        let base = backup_fixture(temp.path(), "base", 5, b"opaque");
        let other = backup_fixture(temp.path(), "other", 42, b"different");

        let composed = compose(
            &base,
            &[("class_level:PVE_TANK".to_string(), other.clone())],
            &temp.path().join("backups"),
            Some("mixed"),
        )
        .unwrap();

        assert_eq!(level_of(&composed), 42);
    }

    #[test]
    fn a_file_the_catalogue_does_not_know_is_carried_over_from_the_base() {
        let temp = tempfile::tempdir().unwrap();
        let base = backup_fixture(temp.path(), "base", 5, b"opaque");
        let other = backup_fixture(temp.path(), "other", 42, b"different");

        let composed = compose(
            &base,
            &[("class_level:PVE_TANK".to_string(), other)],
            &temp.path().join("backups"),
            Some("mixed"),
        )
        .unwrap();

        let files = crate::saves::read_files(&composed).unwrap();
        assert_eq!(files["config/unknown_to_us.cfg"], b"opaque".to_vec());
    }

    #[test]
    fn the_composed_backup_says_what_it_was_made_of() {
        let temp = tempfile::tempdir().unwrap();
        let base = backup_fixture(temp.path(), "base", 5, b"opaque");
        let other = backup_fixture(temp.path(), "other", 42, b"different");

        let composed = compose(
            &base,
            &[("class_level:PVE_TANK".to_string(), other.clone())],
            &temp.path().join("backups"),
            Some("mixed"),
        )
        .unwrap();

        let recorded = crate::saves::composition_of(&composed).unwrap().unwrap();
        assert_eq!(recorded.base, base.created_at);
        assert_eq!(recorded.parts["class_level:PVE_TANK"], other.created_at);
    }

    #[test]
    fn an_unknown_part_id_is_refused_before_anything_is_written() {
        let temp = tempfile::tempdir().unwrap();
        let base = backup_fixture(temp.path(), "base", 5, b"opaque");
        let other = backup_fixture(temp.path(), "other", 42, b"different");
        let backups = temp.path().join("backups");
        let before = crate::saves::list_backups(&backups).unwrap().len();

        let error =
            compose(&base, &[("no_such_part".to_string(), other)], &backups, None).unwrap_err();

        assert!(matches!(error, Error::UnknownPart { .. }), "got {error:?}");
        assert_eq!(crate::saves::list_backups(&backups).unwrap().len(), before);
    }
}
```

Add to `crates/core/src/savedata/mod.rs`:

```rust
pub mod compose;
```

- [ ] **Step 2: Add the remaining error variant and its keys**

In `crates/core/src/error.rs`, add to `enum Error`:

```rust
    /// A part id that this backup does not offer.
    UnknownPart { part: String },
```

In `impl std::fmt::Display for Error`:

```rust
            Error::UnknownPart { part } => {
                i18n::format("error.unknown_part", &[("part", part.clone())])
            }
```

In `crates/core/i18n/en.toml` under `[error]`:

```toml
unknown_part = "'{part}' is not a part this backup offers."
```

In `crates/core/i18n/de.toml` under `[error]`:

```toml
unknown_part = "'{part}' ist kein Bestandteil, den dieses Backup anbietet."
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p sm2-core compose`
Expected: FAIL — `cannot find function compose`.

- [ ] **Step 4: Add the writing half in `saves.rs`**

After `import_archive`, in `crates/core/src/saves.rs`:

```rust
/// Writes an already composed set of files as a new backup.
///
/// The files are laid out in a temporary directory and then go through
/// `write_backup` like everything else, so a composition is written by
/// the same crash-safe sequence as an ordinary backup. The manifest's
/// `source` names the base backup's archive, not the temporary
/// directory, which is gone by the time anyone reads it.
pub fn write_composition(
    files: &BTreeMap<String, Vec<u8>>,
    backup_root: &Path,
    label: Option<&str>,
    source: &Path,
    composition: Composition,
) -> Result<BackupEntry> {
    let staging = tempfile::tempdir().map_err(|e| Error::io(backup_root, e))?;
    for (name, content) in files {
        validate_entry_name(name)?;
        let target = resolve_and_check_target(staging.path(), name)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        write_atomic_bytes(&target, content)?;
    }
    write_backup(
        staging.path(),
        backup_root,
        label,
        source.display().to_string(),
        now_rfc3339(),
        Some(composition),
    )
}

/// What a backup was composed of, or `None` for an ordinary one.
pub fn composition_of(entry: &BackupEntry) -> Result<Option<Composition>> {
    Ok(read_manifest(entry)?.composed_from)
}
```

- [ ] **Step 5: Write the composing half**

Above the tests in `crates/core/src/savedata/compose.rs`:

```rust
//! Composing a new backup out of several existing ones.
//!
//! One backup is the base and supplies everything; a replacement names
//! a part and the backup it should come from instead. Nothing is
//! written until every replacement has gone through, so a refusal
//! anywhere leaves no half-composed backup behind — not even one to
//! clean up.
//!
//! Files the catalogue knows nothing about, and files that no
//! replacement touches, are carried over from the base byte for byte.
//! Only a file that actually changes is encoded again. That keeps the
//! promise the tests check: a composition without a single replacement
//! is the base, file for file.

use crate::error::{Error, Result};
use crate::savedata::catalogue::{self, Documents};
use crate::savedata::merge;
use crate::savedata::ssf1;
use crate::saves::{self, BackupEntry, Composition};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Builds a new backup from `base`, with each named part taken from the
/// backup beside it.
pub fn compose(
    base: &BackupEntry,
    replacements: &[(String, BackupEntry)],
    backup_root: &Path,
    label: Option<&str>,
) -> Result<BackupEntry> {
    saves::verify(base)?;
    let mut files = saves::read_files(base)?;
    let mut documents = catalogue::documents(&files)?;

    // Every source is read once, however many parts come from it.
    let mut sources: BTreeMap<String, Documents> = BTreeMap::new();
    let mut touched: BTreeSet<String> = BTreeSet::new();
    let mut recorded: BTreeMap<String, String> = BTreeMap::new();

    for (part_id, source_entry) in replacements {
        let part = catalogue::part_by_id(&documents, part_id)
            .ok_or_else(|| Error::UnknownPart { part: part_id.clone() })?;

        if !sources.contains_key(&source_entry.created_at) {
            saves::verify(source_entry)?;
            let source_files = saves::read_files(source_entry)?;
            sources.insert(source_entry.created_at.clone(), catalogue::documents(&source_files)?);
        }
        let source = &sources[&source_entry.created_at];

        merge::apply(&mut documents, &part, source)?;
        merge::raise_system_version(&mut documents, part.file, source);
        touched.insert(part.file.to_string());
        recorded.insert(part_id.clone(), source_entry.created_at.clone());
    }

    for name in &touched {
        let json = serde_json::to_vec(&documents[name]).expect("a decoded document serialises");
        let encoded = ssf1::encode(&json);
        // The encoder is exercised on every composition, so it is worth
        // reading the result back before it becomes a backup: a file
        // that does not decode to what went in never reaches the disk.
        let read_back = ssf1::decode(&encoded)?;
        debug_assert_eq!(read_back, json, "the container did not round-trip");
        files.insert(name.clone(), encoded);
    }

    saves::write_composition(
        &files,
        backup_root,
        label,
        &base.archive,
        Composition { base: base.created_at.clone(), parts: recorded },
    )
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p sm2-core compose`
Expected: PASS, five tests.

- [ ] **Step 7: Run the whole suite and clippy**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/savedata crates/core/src/saves.rs \
  crates/core/src/error.rs crates/core/i18n
git commit -m "feat(savedata): compose a backup out of several backups"
```

---

### Task 7: `save parts` on the command line

Lists what a backup offers, with a figure per part so the ids mean
something to a reader.

**Files:**
- Create: `crates/core/src/savedata/summary.rs`
- Modify: `crates/core/src/savedata/mod.rs`
- Modify: `crates/app/src/cli.rs` (`SaveCommand`, `requires_exclusive_access`, `run_save_command`)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`
- Test: in-file `mod tests` in `summary.rs` and in `cli.rs`

**Interfaces:**
- Consumes: `catalogue::{Documents, Part, GROUPS}`.
- Produces:
  - `pub fn summarize(part: &Part, documents: &Documents) -> Option<String>`
  - `pub fn group_name(group: &str) -> String`

- [ ] **Step 1: Write the failing test for the summaries**

Create `crates/core/src/savedata/summary.rs` with the tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::savedata::catalogue::{part_by_id, Documents};
    use serde_json::json;

    fn documents_fixture() -> Documents {
        let mut documents = Documents::new();
        documents.insert(
            "config/user_progression.cfg".to_string(),
            json!({"UserProgression": {"systemVersion": 767, "UserMastery": {"masteryStates": {
                "PVE_TANK": {"json_version": 3, "currentLevel": 21}
            }}}}),
        );
        documents.insert(
            "config/weapon_progression.cfg".to_string(),
            json!({"WeaponProgression": {"systemVersion": 758, "WeaponMastery": {"weaponStates": {
                "hgun_volkite_pistol": {"json_version": 2, "masteryPoints": 4}
            }}}}),
        );
        documents
    }

    #[test]
    fn a_class_part_is_summed_up_by_its_level() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let documents = documents_fixture();
        let part = part_by_id(&documents, "class_level:PVE_TANK").unwrap();
        assert_eq!(summarize(&part, &documents).unwrap(), "level 21");
    }

    #[test]
    fn a_weapon_part_is_summed_up_by_its_mastery_points() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let documents = documents_fixture();
        let part = part_by_id(&documents, "weapon:hgun_volkite_pistol").unwrap();
        assert_eq!(summarize(&part, &documents).unwrap(), "4 mastery points");
    }

    #[test]
    fn a_part_whose_field_is_absent_is_summed_up_as_nothing_rather_than_zero() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let mut documents = documents_fixture();
        documents["config/user_progression.cfg"]["UserProgression"]["UserMastery"]
            ["masteryStates"]["PVE_TANK"]
            .as_object_mut()
            .unwrap()
            .remove("currentLevel");
        let part = part_by_id(&documents, "class_level:PVE_TANK").unwrap();
        assert_eq!(summarize(&part, &documents), None);
    }
}
```

Add to `crates/core/src/savedata/mod.rs`:

```rust
pub mod summary;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sm2-core summary`
Expected: FAIL — `cannot find function summarize`.

- [ ] **Step 3: Write the summaries**

Above the tests in `crates/core/src/savedata/summary.rs`:

```rust
//! One short figure per part, so an id means something to a reader.
//!
//! The rule is a field of the part's own node, named per group. A field
//! that is not there yields nothing at all rather than a zero: a backup
//! from an older build may simply not record it, and "level 0" would be
//! a lie about the save.

use crate::savedata::catalogue::{Documents, Part};

/// Which field sums a group's parts up, and how it is worded.
fn rule(group: &str) -> Option<(&'static str, fn(u64) -> String)> {
    match group {
        "class_level" => Some(("currentLevel", |value| crate::t!("savedata.summary.level", level = value))),
        "weapon" => {
            Some(("masteryPoints", |value| crate::t!("savedata.summary.mastery_points", points = value)))
        }
        "heraldry" => {
            Some(("victoriesCount", |value| crate::t!("savedata.summary.victories", count = value)))
        }
        _ => None,
    }
}

/// A short figure for this part, if there is one worth showing.
pub fn summarize(part: &Part, documents: &Documents) -> Option<String> {
    let (field, word) = rule(part.group)?;
    let node = part.selector.get(documents.get(part.file)?)?;
    Some(word(node.get(field)?.as_u64()?))
}

/// The name of a group, for a list a person reads.
pub fn group_name(group: &str) -> String {
    match group {
        "class_level" => crate::t!("savedata.group.class_level"),
        "armour_loyalist" => crate::t!("savedata.group.armour_loyalist"),
        "armour_chaos" => crate::t!("savedata.group.armour_chaos"),
        "weapon" => crate::t!("savedata.group.weapon"),
        "heraldry" => crate::t!("savedata.group.heraldry"),
        "loadout" => crate::t!("savedata.group.loadout"),
        "challenges" => crate::t!("savedata.group.challenges"),
        "story" => crate::t!("savedata.group.story"),
        "economy" => crate::t!("savedata.group.economy"),
        "pve_state" => crate::t!("savedata.group.pve_state"),
        "horde_mode" => crate::t!("savedata.group.horde_mode"),
        "tutorial" => crate::t!("savedata.group.tutorial"),
        "mutators" => crate::t!("savedata.group.mutators"),
        "achievements" => crate::t!("savedata.group.achievements"),
        other => other.to_string(),
    }
}
```

In `crates/core/i18n/en.toml`, a new section:

```toml
[savedata.summary]
level = "level {level}"
mastery_points = "{points} mastery points"
victories = "{count} victories"

[savedata.group]
class_level = "Class level"
armour_loyalist = "Armour (Loyalist)"
armour_chaos = "Armour (Chaos)"
weapon = "Weapon mastery"
heraldry = "Heraldry"
loadout = "Loadout"
challenges = "Challenges"
story = "Campaign"
economy = "Economy"
pve_state = "Operations"
horde_mode = "Horde mode"
tutorial = "Tutorial hints"
mutators = "Mutator challenges"
achievements = "Achievements"
```

In `crates/core/i18n/de.toml`:

```toml
[savedata.summary]
level = "Stufe {level}"
mastery_points = "{points} Meisterschaftspunkte"
victories = "{count} Siege"

[savedata.group]
class_level = "Klassenstufe"
armour_loyalist = "Rüstung (Loyalist)"
armour_chaos = "Rüstung (Chaos)"
weapon = "Waffenmeisterschaft"
heraldry = "Wappen"
loadout = "Ausrüstungssatz"
challenges = "Herausforderungen"
story = "Kampagne"
economy = "Wirtschaft"
pve_state = "Operationen"
horde_mode = "Hordenmodus"
tutorial = "Tutorial-Hinweise"
mutators = "Mutator-Herausforderungen"
achievements = "Erfolge"
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sm2-core summary`
Expected: PASS, three tests.

- [ ] **Step 5: Add the subcommand**

In `crates/app/src/cli.rs`, add to `enum SaveCommand`:

```rust
    /// Lists the parts a backup offers for composing (default: the
    /// newest one)
    Parts {
        /// 1-based index from `save list` (default: 1, the newest)
        #[arg(long)]
        index: Option<usize>,
        /// Exact timestamp from `save list`
        #[arg(long, conflicts_with = "index")]
        at: Option<String>,
    },
```

In `requires_exclusive_access`, extend the `Save` arm — `Parts` only
reads:

```rust
        Command::Save(sub) => match sub {
            SaveCommand::List | SaveCommand::Parts { .. } => false,
            SaveCommand::Backup { .. }
            | SaveCommand::Restore { .. }
            | SaveCommand::Import { .. }
            | SaveCommand::Rename { .. }
            | SaveCommand::Delete { .. } => true,
        },
```

In `run_save_command`, next to the `SaveCommand::List` arm:

```rust
        SaveCommand::Parts { index, at } => {
            let list = saves::list_backups(&backups)?;
            let entry = resolve_backup_selection(&list, *index, at.as_deref())?;
            let files = saves::read_files(entry)?;
            let documents = catalogue::documents(&files)?;
            let parts = catalogue::parts(&documents);
            if parts.is_empty() {
                println!("{}", t!("cli.save.parts.none"));
                return Ok(());
            }
            for part in &parts {
                let name = summary::group_name(part.group);
                match summary::summarize(part, &documents) {
                    Some(figure) => println!("{}  {name}  {figure}", part.id),
                    None => println!("{}  {name}", part.id),
                }
            }
        }
```

Add the imports `use sm2_core::savedata::{catalogue, summary};` at the
top of `cli.rs`.

In `crates/core/i18n/en.toml`, a new section after `[cli.save.list]`:

```toml
[cli.save.parts]
about = "Lists the parts a backup offers for composing"
none = "This backup holds no parts that can be composed."

[cli.save.parts.arg]
index = "1-based index from `save list` (default: 1, the newest)"
at = "Exact timestamp from `save list`"
```

In `crates/core/i18n/de.toml`:

```toml
[cli.save.parts]
about = "Listet die Bestandteile auf, die ein Backup zum Zusammenstellen anbietet"
none = "Dieses Backup enthält keine zusammenstellbaren Bestandteile."

[cli.save.parts.arg]
index = "1-basierter Index aus `save list` (Standard: 1, das neueste)"
at = "Exakter Zeitstempel aus `save list`"
```

- [ ] **Step 6: Run the command-line tests**

Run: `cargo test -p lina-sm2`
Expected: PASS — in particular `every_command_and_argument_has_a_key`,
which fails if either argument key is missing.

- [ ] **Step 7: Run the whole suite and clippy**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/savedata crates/core/i18n crates/app/src/cli.rs
git commit -m "feat(cli): list the parts a backup offers"
```

---

### Task 8: `save compose` on the command line

**Files:**
- Modify: `crates/app/src/cli.rs` (`SaveCommand`, `requires_exclusive_access`, `run_save_command`, a parser for `--part`)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`
- Test: in-file `mod tests` in `cli.rs`

**Interfaces:**
- Consumes: `savedata::compose::compose`, `resolve_backup_selection`.
- Produces: nothing other tasks use.

- [ ] **Step 1: Write the failing test for the argument parser**

In the existing `mod tests` of `crates/app/src/cli.rs`:

```rust
    #[test]
    fn a_replacement_splits_into_part_and_timestamp() {
        let (part, at) = split_replacement("class_level:PVE_TANK=2026-09-16T16:57:38Z").unwrap();
        assert_eq!(part, "class_level:PVE_TANK");
        assert_eq!(at, "2026-09-16T16:57:38Z");
    }

    #[test]
    fn a_replacement_splits_at_the_first_equals_only() {
        // Part ids hold a colon, timestamps hold colons too — only the
        // `=` separates, and only the first one.
        let (part, at) = split_replacement("loadout:STORY_TITUS=2026-09-16T16:57:38Z").unwrap();
        assert_eq!(part, "loadout:STORY_TITUS");
        assert_eq!(at, "2026-09-16T16:57:38Z");
    }

    #[test]
    fn composing_takes_the_lock_and_listing_parts_does_not() {
        // Composing writes a backup; listing only reads. The match in
        // `requires_exclusive_access` has no `_` arm, so a new
        // subcommand cannot slip through without a decision — this
        // pins the decision itself.
        assert!(requires_exclusive_access(&Command::Save(SaveCommand::Compose {
            base: "2026-09-16T16:57:38Z".to_string(),
            parts: Vec::new(),
            tag: None,
        })));
        assert!(!requires_exclusive_access(&Command::Save(SaveCommand::Parts {
            index: None,
            at: None,
        })));
    }

    #[test]
    fn a_replacement_without_an_equals_is_rejected() {
        assert!(split_replacement("class_level:PVE_TANK").is_err());
    }

    #[test]
    fn a_replacement_with_an_empty_half_is_rejected() {
        assert!(split_replacement("=2026-09-16T16:57:38Z").is_err());
        assert!(split_replacement("class_level:PVE_TANK=").is_err());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p lina-sm2 split_replacement`
Expected: FAIL — `cannot find function split_replacement`.

- [ ] **Step 3: Write the parser**

In `crates/app/src/cli.rs`, next to `resolve_backup_selection`:

```rust
/// Splits `--part <id>=<timestamp>` into its two halves.
///
/// Only the first `=` separates: part ids carry a colon and timestamps
/// carry several, but neither carries an equals sign.
fn split_replacement(argument: &str) -> Result<(&str, &str)> {
    let (part, at) = argument
        .split_once('=')
        .with_context(|| t!("cli.save.compose.malformed_part", argument = argument))?;
    if part.is_empty() || at.is_empty() {
        anyhow::bail!(t!("cli.save.compose.malformed_part", argument = argument));
    }
    Ok((part, at))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p lina-sm2 split_replacement`
Expected: PASS, four tests.

- [ ] **Step 5: Add the subcommand**

In `enum SaveCommand`:

```rust
    /// Composes a new backup: one backup as the base, individual parts
    /// from others
    Compose {
        /// Exact timestamp from `save list` of the backup that supplies
        /// everything not replaced
        #[arg(long)]
        base: String,
        /// A part and the backup it comes from, as `<part>=<timestamp>`;
        /// may be given several times. `save parts` lists the ids.
        #[arg(long = "part")]
        parts: Vec<String>,
        /// Label for the new backup
        #[arg(long)]
        tag: Option<String>,
    },
```

In `requires_exclusive_access`, `Compose` writes a backup:

```rust
            SaveCommand::Backup { .. }
            | SaveCommand::Restore { .. }
            | SaveCommand::Import { .. }
            | SaveCommand::Rename { .. }
            | SaveCommand::Delete { .. }
            | SaveCommand::Compose { .. } => true,
```

In `run_save_command`:

```rust
        SaveCommand::Compose { base, parts, tag } => {
            let list = saves::list_backups(&backups)?;
            let base_entry = resolve_backup_selection(&list, None, Some(base))?.clone();

            let mut replacements = Vec::new();
            for argument in parts {
                let (part, at) = split_replacement(argument)?;
                let source = resolve_backup_selection(&list, None, Some(at))?.clone();
                replacements.push((part.to_string(), source));
            }

            let composed =
                compose::compose(&base_entry, &replacements, &backups, tag.as_deref())?;
            println!(
                "{}",
                t!(
                    "cli.save.compose.done",
                    path = composed.archive.display(),
                    count = replacements.len()
                )
            );
        }
```

Extend the import to `use sm2_core::savedata::{catalogue, compose, summary};`.

In `crates/core/i18n/en.toml`:

```toml
[cli.save.compose]
about = "Composes a new backup out of several backups"
done = "Composed backup written: {path} ({count} parts taken from other backups)"
malformed_part = "'{argument}' is not a replacement. Expected <part>=<timestamp>, for instance class_level:PVE_TANK=2026-09-16T16:57:38Z."

[cli.save.compose.arg]
base = "Exact timestamp from `save list` of the backup that supplies everything not replaced"
parts = "A part and the backup it comes from, as <part>=<timestamp>; may be given several times"
tag = "Label for the new backup"
```

In `crates/core/i18n/de.toml`:

```toml
[cli.save.compose]
about = "Stellt ein neues Backup aus mehreren Backups zusammen"
done = "Zusammengestelltes Backup geschrieben: {path} ({count} Bestandteile aus anderen Backups)"
malformed_part = "'{argument}' ist keine Ersetzung. Erwartet wird <Bestandteil>=<Zeitstempel>, etwa class_level:PVE_TANK=2026-09-16T16:57:38Z."

[cli.save.compose.arg]
base = "Exakter Zeitstempel aus `save list` des Backups, das alles Nichtersetzte liefert"
parts = "Ein Bestandteil und das Backup, aus dem er kommt, als <Bestandteil>=<Zeitstempel>; mehrfach angebbar"
tag = "Etikett für das neue Backup"
```

The key follows the clap field name, not the flag:
`collect_missing_keys` builds it from `arg.get_id()`, which the derive
takes from the field. The field is `parts`, so the key is
`cli.save.compose.arg.parts` even though the flag reads `--part`.

- [ ] **Step 6: Run the whole suite and clippy**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS.

- [ ] **Step 7: Try it by hand against real backups**

```bash
cargo run -- save list
cargo run -- save parts --index 1 | head -30
cargo run -- save compose --base <timestamp-of-base> \
  --part class_level:PVE_TANK=<timestamp-of-other> --tag "merge test"
cargo run -- save list
```

Expected: a new backup appears, and `save parts` on it shows the
Bulwark's level from the other backup. **Do not restore it into the
game yet** — that is the open question the spec names, and it wants a
deliberate test, not a side effect of trying the command.

- [ ] **Step 8: Commit**

```bash
git add crates/app/src/cli.rs crates/core/i18n
git commit -m "feat(cli): compose a backup out of several backups"
```

---

## What this plan does not do

- No interface. The design bundle in `docs/design/` is the plan for
  that, and it sits on the API these eight tasks produce.
- No Steam user profile picker. The design raises it; the spec puts it
  out of scope.
- No verification that the game accepts a merged `systemVersion`. That
  is open question 1 of the spec and needs a deliberate run against the
  installed game, after this is in.
