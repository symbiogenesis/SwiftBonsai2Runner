//! Exact-source, selection and bounded lexical retrieval controls.
use super::*;

#[test]
fn source_revision_hashes_original_utf8_bytes_and_requires_version_labels()
-> Result<(), &'static str> {
    let packet = Source::new("Example", "draft 1", "abc")?.search("abc")?;
    assert_eq!(
        packet.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let changed = Source::new("Example", "draft 1", "abc\r\n")?.search("abc")?;
    assert_ne!(packet.sha256, changed.sha256);
    assert_eq!(packet.version, changed.version);
    for (title, version) in [
        ("", "1"),
        ("Source", ""),
        ("a\nb", "1"),
        ("x", "a\t1"),
        ("Source\n", "1"),
        ("a\u{2028}b", "1"),
        ("x", "a\u{2029}1"),
    ] {
        assert!(Source::new(title, version, "abc").is_err());
    }
    Ok(())
}

#[test]
fn lexical_search_ranks_distinct_words_without_query_or_source_frequency_amplification()
-> Result<(), &'static str> {
    let source = Source::new(
        "Manuscript",
        "draft 7",
        "Garden garden garden garden.\n\nMira entered the garden.\n\nMira waited.\n\nUnrelated ending.",
    )?;
    let packet = source.search("MIRA garden")?;
    assert_eq!(packet.excerpts.len(), 3);
    assert_eq!(packet.excerpts[0].start_line, 3);
    assert_eq!(packet.excerpts[1].start_line, 1);
    assert_eq!(packet.excerpts[2].start_line, 5);
    let repeated = source.search("garden garden garden MIRA MIRA")?;
    assert_eq!(
        packet
            .excerpts
            .iter()
            .map(|excerpt| &excerpt.text)
            .collect::<Vec<_>>(),
        repeated
            .excerpts
            .iter()
            .map(|excerpt| &excerpt.text)
            .collect::<Vec<_>>()
    );
    assert!(
        source
            .search("stone")
            .is_ok_and(|packet| packet.excerpts.is_empty())
    );
    assert!(
        source
            .search("gardener")
            .is_ok_and(|packet| packet.excerpts.is_empty())
    );
    for empty in ["", "!?", "the and it is"] {
        assert!(source.search(empty).is_err());
    }
    Ok(())
}

#[test]
fn chunk_boundaries_do_not_manufacture_whole_word_matches() -> Result<(), &'static str> {
    let leading = format!("{}garden\n\nGarden grows.", "x".repeat(MAX_EXCERPT_CHARS));
    let packet = Source::new("Source", "1", &leading)?.search("garden")?;
    assert_eq!(packet.excerpts.len(), 1);
    assert_eq!(packet.excerpts[0].text, "Garden grows.");

    // No whitespace in the chunk's last half: its final 800-character token is
    // only the prefix of an original 801-character word, not a complete match.
    let term = "g".repeat(800);
    let trailing = format!("{}{}g\n\n{term}", "prefix ".repeat(100), term);
    let packet = Source::new("Source", "1", &trailing)?.search(&term)?;
    assert_eq!(packet.excerpts.len(), 1);
    assert_eq!(packet.excerpts[0].text, term);

    let terms = query_terms("garden")?;
    assert_eq!(score("xgardenx", 1..7, &terms), 0);
    assert_eq!(score(" garden ", 1..7, &terms), 1);
    assert_eq!(score("x garden ", 1..8, &terms), 1);
    assert_eq!(score(" garden x", 0..8, &terms), 1);
    Ok(())
}

#[test]
fn unicode_and_line_endings_remain_exact_with_inclusive_original_line_ranges()
-> Result<(), &'static str> {
    let text = "\r\n  \r\nÉLODIE enters.\r\n花 waits.\r\n\r\nAnother room.\n";
    let packet = Source::new("Original", "3", text)?.search("élodie 花")?;
    assert_eq!(packet.excerpts.len(), 1);
    let excerpt = &packet.excerpts[0];
    assert_eq!((excerpt.start_line, excerpt.end_line), (3, 4));
    assert_eq!(excerpt.text, "ÉLODIE enters.\r\n花 waits.\r\n");
    assert!(text.contains(&excerpt.text));
    let rendered = packet.render(&[0])?;
    assert!(rendered.contains("Version: 3\nSource SHA-256:"));
    assert!(rendered.contains("[Source excerpt, lines 3-4]"));
    assert!(rendered.contains(&excerpt.text));
    Ok(())
}

#[test]
fn long_paragraphs_partition_every_byte_at_unicode_boundaries_without_truncation()
-> Result<(), &'static str> {
    let text = format!("{}\n{}", "é garden ".repeat(400), "花".repeat(1900));
    let source = Source::new("Long source", "v1", &text)?;
    let mut covered = String::new();
    for passage in &source.passages {
        let chunk = &source.text[passage.bytes.clone()];
        assert!(chunk.chars().count() <= MAX_EXCERPT_CHARS);
        assert!(passage.start_line >= 1);
        assert!(passage.end_line >= passage.start_line);
        covered.push_str(chunk);
    }
    assert_eq!(covered, text);
    let packet = source.search("garden 花")?;
    assert!(!packet.excerpts.is_empty());
    assert!(
        packet
            .excerpts
            .iter()
            .all(|excerpt| text.contains(&excerpt.text))
    );
    Ok(())
}

#[test]
fn source_query_and_fragment_limits_reject_intact() -> Result<(), &'static str> {
    assert!(Source::new("Source", "v1", " \r\n").is_err());
    assert!(Source::new("Source", "v1", &"x".repeat(MAX_SOURCE_BYTES + 1)).is_err());
    assert!(Source::new("Source", "v1", &"花".repeat(MAX_SOURCE_BYTES / 3 + 1)).is_err());
    assert!(Source::new(&"x".repeat(MAX_TITLE_CHARS + 1), "1", "source").is_err());
    assert!(Source::new("Source", &"x".repeat(MAX_VERSION_CHARS + 1), "source").is_err());
    assert!(Source::new("Source", "1", &"x\n\n".repeat(MAX_PASSAGES + 1)).is_err());
    let source = Source::new("Source", "v1", "garden")?;
    assert!(source.search(&"x".repeat(MAX_QUERY_CHARS + 1)).is_err());
    let too_many = (0..=MAX_QUERY_TERMS)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(source.search(&too_many).is_err());
    Ok(())
}

#[test]
fn packet_renders_only_explicit_selection_and_rejects_duplicate_or_invalid_indices()
-> Result<(), &'static str> {
    let packet = Source::new("Novel", "author draft", "Garden first.\n\nGarden second.")?
        .search("garden")?;
    assert!(packet.render(&[]).is_err());
    assert!(packet.render(&[0, 0]).is_err());
    assert!(packet.render(&[2]).is_err());
    assert!(packet.render(&[usize::MAX]).is_err());
    let rendered = packet.render(&[1])?;
    assert!(rendered.contains("Garden second."));
    assert!(!rendered.contains("Garden first."));
    assert!(rendered.contains("Reference material"));
    assert!(rendered.ends_with("[End scene packet]"));
    Ok(())
}

#[test]
fn result_count_and_complete_packet_bounds_include_provenance() -> Result<(), &'static str> {
    let text = (0..20)
        .map(|index| format!("Garden {index}.\n\n"))
        .collect::<String>();
    let packet = Source::new("Source", "1", &text)?.search("garden")?;
    assert_eq!(packet.excerpts.len(), MAX_EXCERPTS);
    assert_eq!(packet.excerpts[7].text, "Garden 7.\n");
    let full = format!("garden {}", "x".repeat(1493));
    let text = (0..8).map(|_| format!("{full}\n\n")).collect::<String>();
    let packet = Source::new("Source", "1", &text)?.search("garden")?;
    assert_eq!(packet.excerpts.len(), MAX_EXCERPTS);
    assert!(packet.render(&(0..8).collect::<Vec<_>>()).is_err());
    let fewer = packet.render(&(0..7).collect::<Vec<_>>())?;
    assert!(fewer.chars().count() <= MAX_PACKET_CHARS);
    Ok(())
}
