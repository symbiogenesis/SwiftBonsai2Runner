//! Bounded byte-level BPE for the pinned Qwen3-ASR tokenizer.
//!
//! This implements the Qwen2 pretokenizer, first-occurrence vocabulary/merge
//! policy and literal byte decoding from the source in `rust-tokenizer.json`.
//! It is independent of the currently selected native inference engine.
#![forbid(unsafe_code)]

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    io::{self, Read, Seek},
};

#[path = "tokenizer/scanner.rs"]
mod scanner;
#[path = "tokenizer/unicode.rs"]
mod unicode;

const MAX_INPUT: usize = 32 * 1024;
const MAX_IDS: usize = 8192;
const MAX_ENTRIES: usize = 262_144;
const MAX_SPELLING: usize = 4096;
const MAX_TABLE: usize = 64 * 1024 * 1024;
const MAX_PROBES: usize = 64 * 1024 * 1024;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid Qwen3-ASR tokenizer input or tables.",
    )
}

fn byte_glyph(byte: u8) -> char {
    if matches!(byte, b'!'..=b'~' | 0xa1..=0xac | 0xae..=0xff) {
        return char::from(byte);
    }
    let offset = match byte {
        0..=32 => u32::from(byte),
        127..=160 => u32::from(byte) - 94,
        173 => 67,
        _ => 0,
    };
    char::from_u32(256 + offset).unwrap_or('\0')
}

fn encoded(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| byte_glyph(*byte)).collect()
}

fn raw_byte(glyph: char) -> Option<u8> {
    let cp = u32::from(glyph);
    match cp {
        0x21..=0x7e | 0xa1..=0xac | 0xae..=0xff => u8::try_from(cp).ok(),
        256..=288 => u8::try_from(cp - 256).ok(),
        289..=322 => u8::try_from(cp - 256 + 94).ok(),
        323 => Some(173),
        _ => None,
    }
}

/// Immutable Qwen2/GPT-2 byte vocabulary and ranked merge table.
pub struct Tokenizer {
    tokens: Vec<String>,
    vocabulary: HashMap<String, u32>,
    merges: HashMap<String, usize>,
    eos: u32,
}

impl Tokenizer {
    /// Number of original GGUF vocabulary slots, including duplicate spellings.
    #[must_use]
    pub const fn vocabulary_size(&self) -> usize {
        self.tokens.len()
    }
    /// Admit the complete verified GGUF index and extract its tokenizer.
    ///
    /// The caller retains artifact digest verification and the immutable file
    /// guard. Only absent, empty or `qwen2` pretokenizer metadata is supported.
    /// No model weights are read and no inference is performed.
    ///
    /// # Errors
    /// Rejects malformed GGUF, unsupported metadata, invalid tables or bounds.
    pub fn load(source: &mut (impl Read + Seek), length: u64) -> io::Result<Self> {
        crate::intake::tokenizer(source, length).map(|(tokenizer, _)| tokenizer)
    }

    /// Construct from GGUF-order spellings and `left<space>right` merge lines.
    ///
    /// Duplicate spellings and merge pairs retain their first occurrence, as in
    /// the pinned native loader. Special-looking text is encoded as ordinary
    /// text; prompt construction resolves special IDs separately.
    ///
    /// # Errors
    /// Rejects empty/oversized tables, invalid EOS, malformed merges, spelling
    /// limits and aggregate table bytes exceeding 64 MiB.
    pub fn from_tables(tokens: Vec<String>, lines: Vec<String>, eos: u32) -> io::Result<Self> {
        if tokens.is_empty()
            || tokens.len() > MAX_ENTRIES
            || lines.is_empty()
            || lines.len() > MAX_ENTRIES
            || usize::try_from(eos).map_err(|_| invalid())? >= tokens.len()
        {
            return Err(invalid());
        }
        let mut charged = 0_usize;
        let mut vocabulary = HashMap::new();
        vocabulary
            .try_reserve(tokens.len())
            .map_err(io::Error::other)?;
        for (id, piece) in tokens.iter().enumerate() {
            charge(&mut charged, piece)?;
            vocabulary
                .entry(piece.clone())
                .or_insert(u32::try_from(id).map_err(|_| invalid())?);
        }
        let mut merges = HashMap::new();
        merges.try_reserve(lines.len()).map_err(io::Error::other)?;
        for (rank, line) in lines.into_iter().enumerate() {
            charge(&mut charged, &line)?;
            let split = line
                .as_bytes()
                .iter()
                .enumerate()
                .skip(1)
                .find_map(|(offset, byte)| (*byte == b' ').then_some(offset))
                .ok_or_else(invalid)?;
            if line.contains('\u{1f}') {
                return Err(invalid());
            }
            let mut key = line;
            key.replace_range(split..=split, "\u{1f}");
            merges.entry(key).or_insert(rank);
        }
        Ok(Self {
            tokens,
            vocabulary,
            merges,
            eos,
        })
    }

    /// Resolve a literal encoded vocabulary spelling, retaining first occurrence.
    #[must_use]
    pub fn find(&self, piece: &str) -> Option<u32> {
        self.vocabulary.get(piece).copied()
    }

    /// The model's validated end-of-sequence token.
    #[must_use]
    pub const fn eos(&self) -> u32 {
        self.eos
    }

    /// Encode at most 32 KiB of UTF-8 with ranked, leftmost-tie BPE.
    ///
    /// # Errors
    /// Rejects missing fallback bytes, more than 8,192 output tokens, exhausted
    /// merge-work allowance or input limits. No partial IDs are published.
    pub fn encode(&self, text: &str) -> io::Result<Vec<u32>> {
        if text.len() > MAX_INPUT {
            return Err(invalid());
        }
        let mut output = Vec::new();
        output
            .try_reserve(text.len().min(MAX_IDS))
            .map_err(io::Error::other)?;
        let mut probes = MAX_PROBES;
        for word in scanner::pretokens(text) {
            self.encode_word(word.as_bytes(), &mut output, &mut probes)?;
        }
        Ok(output)
    }

    fn encode_word(
        &self,
        word: &[u8],
        output: &mut Vec<u32>,
        probes: &mut usize,
    ) -> io::Result<()> {
        let mut symbols: Vec<_> = (0..word.len())
            .map(|start| Symbol {
                start,
                end: start + 1,
                previous: start.checked_sub(1),
                next: (start + 1 < word.len()).then_some(start + 1),
            })
            .collect();
        let mut queue = BinaryHeap::new();
        for left in 0..word.len().saturating_sub(1) {
            self.enqueue(word, &symbols, left, &mut queue, probes)?;
        }
        while let Some(Reverse((_, left, right, middle, end))) = queue.pop() {
            if symbols[left].end != middle
                || symbols[right].start != middle
                || symbols[right].end != end
                || symbols[left].next != Some(right)
            {
                continue;
            }
            symbols[left].end = end;
            symbols[left].next = symbols[right].next;
            symbols[right].end = symbols[right].start;
            if let Some(next) = symbols[left].next {
                symbols[next].previous = Some(left);
            }
            if let Some(previous) = symbols[left].previous {
                self.enqueue(word, &symbols, previous, &mut queue, probes)?;
            }
            self.enqueue(word, &symbols, left, &mut queue, probes)?;
        }
        for symbol in symbols.iter().filter(|symbol| symbol.end > symbol.start) {
            if let Some(id) = self.find(&encoded(&word[symbol.start..symbol.end])) {
                push_id(output, id)?;
            } else {
                let mut glyph = [0; 4];
                for byte in &word[symbol.start..symbol.end] {
                    let id = self
                        .find(byte_glyph(*byte).encode_utf8(&mut glyph))
                        .ok_or_else(invalid)?;
                    push_id(output, id)?;
                }
            }
        }
        Ok(())
    }

    fn enqueue(
        &self,
        word: &[u8],
        symbols: &[Symbol],
        left: usize,
        queue: &mut Queue,
        probes: &mut usize,
    ) -> io::Result<()> {
        let Some(right) = symbols[left].next else {
            return Ok(());
        };
        let (a, b) = (&symbols[left], &symbols[right]);
        *probes = probes.checked_sub(b.end - a.start).ok_or_else(invalid)?;
        let mut key = encoded(&word[a.start..a.end]);
        key.push('\u{1f}');
        key.extend(word[b.start..b.end].iter().map(|byte| byte_glyph(*byte)));
        if let Some(rank) = self.merges.get(&key) {
            queue.push(Reverse((*rank, left, right, a.end, b.end)));
        }
        Ok(())
    }

    /// Decode literal bytes without stripping control/EOS spellings.
    ///
    /// Out-of-range IDs are skipped to preserve the pinned native decoder.
    /// Unknown glyphs pass through as their original UTF-8. Byte fragments may
    /// span token boundaries and need not independently form valid UTF-8.
    ///
    /// # Errors
    /// Rejects more than 8,192 IDs or decoded bytes exceeding 32 KiB.
    pub fn decode_bytes(&self, ids: &[u32]) -> io::Result<Vec<u8>> {
        if ids.len() > MAX_IDS {
            return Err(invalid());
        }
        let mut output = Vec::new();
        for id in ids {
            let Some(piece) = usize::try_from(*id).ok().and_then(|id| self.tokens.get(id)) else {
                continue;
            };
            for glyph in piece.chars() {
                let mut bytes = [0; 4];
                let encoded = glyph.encode_utf8(&mut bytes).as_bytes();
                let byte = raw_byte(glyph);
                let addition = byte.map_or(encoded.len(), |_| 1);
                if output.len() + addition > MAX_INPUT {
                    return Err(invalid());
                }
                output.try_reserve(addition).map_err(io::Error::other)?;
                if let Some(byte) = byte {
                    output.push(byte);
                } else {
                    output.extend_from_slice(encoded);
                }
            }
        }
        Ok(output)
    }

    /// Decode a complete UTF-8 sequence, rejecting incomplete byte fragments.
    ///
    /// # Errors
    /// Returns the limits from [`Self::decode_bytes`] or invalid UTF-8.
    pub fn decode(&self, ids: &[u32]) -> io::Result<String> {
        String::from_utf8(self.decode_bytes(ids)?).map_err(|_| invalid())
    }
}

struct Symbol {
    start: usize,
    end: usize,
    previous: Option<usize>,
    next: Option<usize>,
}
type Queue = BinaryHeap<Reverse<(usize, usize, usize, usize, usize)>>;

fn charge(total: &mut usize, piece: &str) -> io::Result<()> {
    if piece.len() > MAX_SPELLING {
        return Err(invalid());
    }
    *total = total
        .checked_add(piece.len())
        .filter(|bytes| *bytes <= MAX_TABLE)
        .ok_or_else(invalid)?;
    Ok(())
}

fn push_id(output: &mut Vec<u32>, id: u32) -> io::Result<()> {
    if output.len() >= MAX_IDS {
        return Err(invalid());
    }
    output.push(id);
    Ok(())
}

#[cfg(test)]
#[path = "tokenizer/tests.rs"]
mod tests;
