//! Retained loader/lifetime assertions against serving's safe Rust intake.
#![forbid(unsafe_code)]

use std::{
    cell::{Cell, RefCell},
    io,
    panic::{AssertUnwindSafe, catch_unwind},
};

use crate::gguf::invalid;

use super::{MERGES, Metadata, SPECIAL_KEYS, TOKENS, Tokenizer, candidate, tables};

#[derive(Clone)]
struct Fixture {
    tokens: Vec<Option<Vec<u8>>>,
    merges: Vec<Option<Vec<u8>>>,
    missing: Option<usize>,
    oversized: bool,
    array_type: u32,
    element_type: u32,
    special_type: u32,
    special_id: u32,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            tokens: [b"a".as_slice(), b"a", b"<eos>"]
                .map(|text| Some(text.to_vec()))
                .to_vec(),
            merges: [b"a a".as_slice(), b"a a"]
                .map(|text| Some(text.to_vec()))
                .to_vec(),
            missing: None,
            oversized: false,
            array_type: 9,
            element_type: 8,
            special_type: 4,
            special_id: 1,
        }
    }
}

std::thread_local! {
    pub(super) static OWNER_COUNTS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    pub(super) static REJECT: Cell<u8> = const { Cell::new(0) };
    static GGUF_COUNTS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    static READS: RefCell<Vec<(usize, usize)>> = const { RefCell::new(Vec::new()) };
    static RESULT_COUNTS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

struct MockMetadata(Fixture);

impl MockMetadata {
    fn open(path: &[u8], fixture: Fixture) -> io::Result<Self> {
        if path != b"fixture" {
            return Err(invalid());
        }
        GGUF_COUNTS.with(|counts| {
            let (opened, closed) = counts.get();
            counts.set((opened + 1, closed));
        });
        Ok(Self(fixture))
    }
}

impl Drop for MockMetadata {
    fn drop(&mut self) {
        GGUF_COUNTS.with(|counts| {
            let (opened, closed) = counts.get();
            counts.set((opened, closed + 1));
        });
    }
}

impl Metadata for MockMetadata {
    fn find(&self, name: &[u8]) -> Option<usize> {
        let index = [TOKENS, MERGES]
            .into_iter()
            .chain(SPECIAL_KEYS)
            .position(|key| key == name)?;
        (self.0.missing != Some(index)).then_some(index)
    }

    fn string_count(&self, key: usize) -> io::Result<usize> {
        if self.0.array_type != 9 || self.0.element_type != 8 {
            return Err(invalid());
        }
        if self.0.oversized {
            return Ok(usize::try_from(1_u64 << 32).unwrap_or(usize::MAX));
        }
        Ok(if key == 0 {
            self.0.tokens.len()
        } else {
            self.0.merges.len()
        })
    }

    fn string(&self, key: usize, index: usize) -> io::Result<&[u8]> {
        READS.with(|reads| reads.borrow_mut().push((key, index)));
        let values = if key == 0 {
            &self.0.tokens
        } else {
            &self.0.merges
        };
        values
            .get(index)
            .and_then(Option::as_deref)
            .ok_or_else(invalid)
    }

    fn u32_value(&self, _: usize) -> io::Result<u32> {
        if self.0.special_type == 4 {
            Ok(self.0.special_id)
        } else {
            Err(invalid())
        }
    }
}

#[derive(Default)]
struct Slot(Option<Box<Tokenizer>>);

impl Slot {
    fn load(&mut self, path: &[u8], fixture: Fixture) -> io::Result<()> {
        let metadata = MockMetadata::open(path, fixture)?;
        let replacement = Box::new(candidate(&metadata)?);
        self.0 = Some(replacement);
        Ok(())
    }

    fn owner(&self) -> io::Result<&Tokenizer> {
        self.0.as_deref().ok_or_else(invalid)
    }
}

struct Fault;

impl Fault {
    fn at(stage: u8) -> Self {
        REJECT.with(|reject| reject.set(stage));
        Self
    }
}

impl Drop for Fault {
    fn drop(&mut self) {
        REJECT.with(|reject| reject.set(0));
    }
}

fn balanced_metadata() {
    GGUF_COUNTS.with(|counts| {
        let (opened, closed) = counts.get();
        assert_eq!(opened, closed);
    });
}

fn reset() {
    OWNER_COUNTS.with(|counts| counts.set((0, 0)));
    RESULT_COUNTS.with(|counts| counts.set((0, 0)));
    GGUF_COUNTS.with(|counts| counts.set((0, 0)));
    REJECT.with(|reject| reject.set(0));
    READS.with(|reads| reads.borrow_mut().clear());
}

#[test]
fn rust_gguf_loader_owns_metadata_candidates_and_transactional_replacement() -> io::Result<()> {
    reset();
    let mut slot = Slot::default();
    assert!(slot.load(b"missing", Fixture::default()).is_err());
    assert!(slot.0.is_none());
    assert_eq!(OWNER_COUNTS.with(Cell::get), (0, 0));
    slot.load(b"fixture", Fixture::default())?;
    let original = std::ptr::from_ref(slot.owner()?);
    assert_eq!(OWNER_COUNTS.with(Cell::get), (1, 0));
    assert_eq!(
        READS.with(|reads| reads.borrow().clone()),
        [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1)]
    );
    assert_eq!(slot.owner()?.token_id_for_test(b"a"), Some(1));
    assert_eq!(slot.owner()?.token_id_for_test(b"<eos>"), Some(2));
    assert_eq!(slot.owner()?.merge_rank_for_test(b"a a"), Some(1));
    assert_eq!(slot.owner()?.encode(b"a", false)?, [1]);
    balanced_metadata();
    for stage in 1..=5 {
        let before = OWNER_COUNTS.with(Cell::get);
        let fault = Fault::at(stage);
        assert!(slot.load(b"fixture", Fixture::default()).is_err());
        assert_eq!(std::ptr::from_ref(slot.owner()?), original);
        let created = usize::from(stage != 1);
        assert_eq!(
            OWNER_COUNTS.with(Cell::get),
            (before.0 + created, before.1 + created)
        );
        balanced_metadata();
        drop(fault);
        assert_eq!(slot.owner()?.encode(b"a", false)?, [1]);
    }
    for mode in 0..11 {
        let mut fixture = Fixture::default();
        match mode {
            0 => fixture.missing = Some(0),
            1 => fixture.missing = Some(1),
            2 => fixture.tokens[0] = None,
            3 => fixture.merges[0] = None,
            4 => fixture.oversized = true,
            5 => fixture.array_type = 4,
            6 => fixture.element_type = 4,
            7 => fixture.missing = Some(2),
            8 => fixture.special_type = 8,
            9 => fixture.special_id = u32::MAX,
            _ => fixture.special_id = i32::MAX.cast_unsigned() + 1,
        }
        assert!(slot.load(b"fixture", fixture).is_err());
        assert_eq!(std::ptr::from_ref(slot.owner()?), original);
        assert_eq!(slot.owner()?.encode(b"a", false)?, [1]);
        balanced_metadata();
    }
    let before = OWNER_COUNTS.with(Cell::get);
    slot.load(b"fixture", Fixture::default())?;
    assert_ne!(std::ptr::from_ref(slot.owner()?), original);
    assert_eq!(OWNER_COUNTS.with(Cell::get), (before.0 + 1, before.1 + 1));
    balanced_metadata();
    drop(slot);
    OWNER_COUNTS.with(|counts| assert_eq!(counts.get().0, counts.get().1));
    Ok(())
}

#[test]
fn rust_special_intake_preserves_order_signed_ids_errors_and_cleanup() -> io::Result<()> {
    reset();
    let metadata = MockMetadata::open(b"fixture", Fixture::default())?;
    let mut owner = tables(&metadata)?;
    let keys = [(b"first".as_slice(), 1_u32), (b"invalid", u32::MAX)];
    let mut observed = Vec::new();
    for name in [b"missing".as_slice(), b"first", b"invalid", b"first"] {
        if let Some((_, bits)) = keys.iter().find(|(key, _)| *key == name) {
            let id = bits.cast_signed();
            observed.push(id);
            owner.add_special_id(id)?;
        }
    }
    assert_eq!(observed, [1, -1, 1]);
    owner.add_special(b"<voice>", 2)?;
    assert_eq!(owner.encode(b"<voice>", false)?, [2]);
    assert!(owner.add_special(b"", 0).is_err());
    assert!(owner.add_special(&vec![b'a'; 4097], 0).is_err());
    drop(owner);
    drop(metadata);
    balanced_metadata();
    OWNER_COUNTS.with(|counts| assert_eq!(counts.get().0, counts.get().1));
    Ok(())
}

struct Encoding(Vec<i32>);

impl Drop for Encoding {
    fn drop(&mut self) {
        RESULT_COUNTS.with(|counts| {
            let (created, freed) = counts.get();
            counts.set((created, freed + 1));
        });
    }
}

fn encode_to(
    owner: &Tokenizer,
    text: &[u8],
    eos: bool,
    copy: impl FnOnce(&[i32]) -> bool,
) -> io::Result<bool> {
    let ids = Encoding(owner.encode(text, eos)?);
    RESULT_COUNTS.with(|counts| {
        let (created, freed) = counts.get();
        counts.set((created + 1, freed));
    });
    Ok(copy(&ids.0))
}

#[test]
#[expect(
    clippy::panic,
    reason = "Deliberate safe Rust unwind checks result cleanup."
)]
fn rust_encoding_owns_results_through_copy_failure_empty_output_and_unwind() -> io::Result<()> {
    reset();
    let fixture = Fixture {
        tokens: [
            b"a".as_slice(),
            b"b",
            "\u{100}".as_bytes(),
            b"<|endoftext|>",
        ]
        .map(|text| Some(text.to_vec()))
        .to_vec(),
        merges: Vec::new(),
        ..Fixture::default()
    };
    let metadata = MockMetadata::open(b"fixture", fixture)?;
    let owner = candidate(&metadata)?;
    for (text, eos, expected) in [
        (b"a\0b".as_slice(), false, vec![0, 2, 1]),
        (b"", false, vec![]),
        (b"", true, vec![3]),
    ] {
        assert!(encode_to(&owner, text, eos, |ids| {
            assert_eq!(ids, expected);
            true
        })?);
    }
    assert!(!encode_to(&owner, b"a", false, |_| false)?);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = encode_to(&owner, b"a", false, |_| panic!("injected copy failure"));
        }))
        .is_err()
    );
    assert!(encode_to(&owner, &[0xff], false, |_| true).is_err());
    RESULT_COUNTS.with(|counts| assert_eq!(counts.get().0, counts.get().1));
    let ids = owner.encode(b"ab", false)?;
    drop(owner);
    assert_eq!(ids, [0, 1]);
    drop(metadata);
    balanced_metadata();
    OWNER_COUNTS.with(|counts| assert_eq!(counts.get().0, counts.get().1));
    Ok(())
}

fn gguf_string(output: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    output.extend(
        u64::try_from(bytes.len())
            .map_err(io::Error::other)?
            .to_le_bytes(),
    );
    output.extend(bytes);
    Ok(())
}

fn tiny_gguf(tokens: &[&[u8]], merges: &[&[u8]]) -> io::Result<Vec<u8>> {
    let mut bytes = b"GGUF".to_vec();
    bytes.extend(3_u32.to_le_bytes());
    bytes.extend(0_u64.to_le_bytes());
    bytes.extend(7_u64.to_le_bytes());
    for (name, values) in [(TOKENS, tokens), (MERGES, merges)] {
        gguf_string(&mut bytes, name)?;
        bytes.extend(9_u32.to_le_bytes());
        bytes.extend(8_u32.to_le_bytes());
        bytes.extend(
            u64::try_from(values.len())
                .map_err(io::Error::other)?
                .to_le_bytes(),
        );
        for value in values {
            gguf_string(&mut bytes, value)?;
        }
    }
    for name in SPECIAL_KEYS {
        gguf_string(&mut bytes, name)?;
        bytes.extend(4_u32.to_le_bytes());
        bytes.extend(0_u32.to_le_bytes());
    }
    while !bytes.len().is_multiple_of(32) {
        bytes.push(0);
    }
    Ok(bytes)
}

#[test]
fn real_rust_gguf_reader_reloads_owned_tables_without_model_weights() -> io::Result<()> {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct File(std::path::PathBuf);
    impl Drop for File {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    reset();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "Bonsai-RustTokenizer-{}-{stamp}.gguf",
        std::process::id()
    ));
    let _file = File(path.clone());
    fs::write(
        &path,
        tiny_gguf(&[b"a", b"a", b"<|endoftext|>"], &[b"a a", b"a a"])?,
    )?;
    let mut owner = Box::new(Tokenizer::load(&path)?);
    assert_eq!(owner.encode(b"a", true)?, [1, 2]);
    let original = std::ptr::from_ref(owner.as_ref());
    fs::write(&path, b"invalid GGUF")?;
    assert!(Tokenizer::load(&path).is_err());
    assert_eq!(std::ptr::from_ref(owner.as_ref()), original);
    assert_eq!(owner.encode(b"a", true)?, [1, 2]);
    fs::write(&path, tiny_gguf(&[b"b"], &[])?)?;
    let replacement = Box::new(Tokenizer::load(&path)?);
    owner = replacement;
    assert_ne!(std::ptr::from_ref(owner.as_ref()), original);
    assert_eq!(owner.encode(b"b", false)?, [0]);
    drop(owner);
    OWNER_COUNTS.with(|counts| assert_eq!(counts.get().0, counts.get().1));
    Ok(())
}
