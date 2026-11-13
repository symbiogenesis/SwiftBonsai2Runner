//! Literal Qwen2 splits and miniature ranked-merge controls; no native execution.

use super::{Tokenizer, byte_glyph, raw_byte, scanner, unicode};
use std::io;

fn fixture(extra: &[&str], merges: &[&str]) -> io::Result<Tokenizer> {
    let mut tokens: Vec<_> = (0..=u8::MAX)
        .map(|byte| byte_glyph(byte).to_string())
        .collect();
    tokens.extend(extra.iter().map(|piece| (*piece).to_owned()));
    Tokenizer::from_tables(
        tokens,
        merges.iter().map(|line| (*line).to_owned()).collect(),
        0,
    )
}

#[test]
fn rust_asr_tokenizer_literal_qwen2_unicode_splits() {
    for (text, expected) in [
        (
            "I'm I'LL 'there",
            vec!["I", "'m", " I", "'LL", " '", "there"],
        ),
        ("a123 ٤² b", vec!["a", "1", "2", "3", " ", "٤", "²", " b"]),
        (
            "  界\u{a0}é\r\n \t!\n",
            vec![" ", " 界", "\u{a0}é", "\r\n", " ", "\t", "!\n"],
        ),
        ("a\u{301}b🙂z", vec!["a", "\u{301}b", "🙂z"]),
        ("a\u{200b}b", vec!["a", "\u{200b}b"]),
    ] {
        assert_eq!(
            scanner::pretokens(text).collect::<Vec<_>>(),
            expected,
            "{text:?}"
        );
    }
}

#[test]
fn rust_asr_tokenizer_pinned_unicode_categories_and_byte_map() {
    assert_eq!(unicode::flags('A') & 6, 4);
    assert_eq!(unicode::flags('²') & 6, 2);
    assert_eq!(unicode::flags('\u{301}') & 0xff, 0x10);
    assert_eq!(unicode::flags('\u{a0}') & 0x100, 0x100);
    assert_eq!(unicode::flags('\u{200b}') & 0x100, 0);
    for (byte, glyph) in [
        (0, 'Ā'),
        (10, 'Ċ'),
        (32, 'Ġ'),
        (127, 'ġ'),
        (160, 'ł'),
        (173, 'Ń'),
        (255, 'ÿ'),
    ] {
        assert_eq!(byte_glyph(byte), glyph);
        assert_eq!(raw_byte(glyph), Some(byte));
    }
    assert_eq!(raw_byte('界'), None);
}

#[test]
fn rust_asr_tokenizer_ranked_merges_and_stale_neighbors() -> io::Result<()> {
    // b+c wins first although a+b is farther left; the stale a+b entry must
    // disappear. The new a+bc pair then produces the literal vocabulary ID.
    let tokenizer = fixture(&["ab", "bc", "abc"], &["b c", "a b", "a bc"])?;
    assert_eq!(tokenizer.encode("abc")?, [258]);
    assert_eq!(tokenizer.encode("abcabc")?, [258, 258]);
    let tokenizer = fixture(&["ab", "aba"], &["a b", "ab a"])?;
    assert_eq!(tokenizer.encode("ababa")?, [256, 257]);
    Ok(())
}

#[test]
fn rust_asr_tokenizer_first_duplicates_and_missing_merged_piece() -> io::Result<()> {
    let tokenizer = fixture(&["ab", "ab", "bc"], &["a b", "b c", "a b"])?;
    assert_eq!(tokenizer.find("ab"), Some(256));
    assert_eq!(tokenizer.encode("abc")?, [256, u32::from(b'c')]);
    let tokenizer = fixture(&[], &["a b"])?;
    assert_eq!(tokenizer.encode("ab")?, [u32::from(b'a'), u32::from(b'b')]);
    Ok(())
}

#[test]
fn rust_asr_tokenizer_digits_and_special_text_do_not_bypass_bpe() -> io::Result<()> {
    let tokenizer = fixture(&["12", "<|im_end|>"], &["1 2"])?;
    assert_eq!(tokenizer.encode("12")?, [49, 50]);
    assert_eq!(
        tokenizer.encode("<|im_end|>")?,
        [60, 124, 105, 109, 95, 101, 110, 100, 124, 62]
    );
    assert_eq!(tokenizer.decode(&[257])?, "<|im_end|>");
    Ok(())
}

#[test]
fn rust_asr_tokenizer_utf8_fragments_and_unknown_glyph_passthrough() -> io::Result<()> {
    let tokenizer = fixture(&["界"], &["a b"])?;
    assert_eq!(tokenizer.encode("界")?, [231, 149, 140]);
    assert_eq!(tokenizer.decode(&[231, 149, 140])?, "界");
    assert_eq!(tokenizer.decode_bytes(&[231])?, [0xe7]);
    assert!(tokenizer.decode(&[231]).is_err());
    assert_eq!(tokenizer.decode(&[u32::MAX, 256])?, "界");
    assert_eq!(tokenizer.decode_bytes(&[0, 173])?, [0, 173]);
    Ok(())
}

#[test]
fn rust_asr_tokenizer_bounds_and_bad_tables_publish_nothing() -> io::Result<()> {
    assert!(Tokenizer::from_tables(vec!["a".into()], vec!["ab".into()], 0).is_err());
    assert!(Tokenizer::from_tables(vec!["a".into()], vec!["a b".into()], 1).is_err());
    assert!(Tokenizer::from_tables(vec!["a".into()], vec!["a\u{1f} b".into()], 0).is_err());
    assert!(Tokenizer::from_tables(vec!["a".into()], Vec::new(), 0).is_err());
    let tokenizer = fixture(&[], &["a b"])?;
    assert!(tokenizer.encode(&"a".repeat(8193)).is_err());
    assert!(tokenizer.encode(&"a".repeat(32769)).is_err());
    assert!(tokenizer.decode_bytes(&vec![0; 8193]).is_err());
    assert_eq!(tokenizer.encode("")?, Vec::<u32>::new());
    assert_eq!(tokenizer.eos(), 0);
    Ok(())
}
