use super::*;
use serde_json::json;

#[test]
fn automatic_threads_preserve_small_hosts_and_bounded_grid() -> crate::Result<()> {
    for (available, expected) in [
        (1, vec![1]),
        (2, vec![1, 2]),
        (3, vec![1, 3]),
        (4, vec![2, 4]),
        (12, vec![6, 10, 12]),
        (256, vec![128, 254, 256]),
    ] {
        assert_eq!(thread_grid(available)?, expected);
    }
    assert!(thread_grid(0).is_err());
    assert!(thread_grid(257).is_err());
    Ok(())
}

fn fixture() -> Vec<u8> {
    // Independent canonical wire bytes, including the two signed PCM extrema.
    b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\xc0\x5d\0\0\x80\xbb\0\0\x02\0\x10\0data\x04\0\0\0\0\x80\xff\x7f".to_vec()
}

#[test]
fn pcm_conversion_and_wave_admission_preserve_exact_duration_and_format() -> crate::Result<()> {
    let expected = fixture();
    assert_eq!(wave(Format::Pcm, &expected[44..])?, expected);
    assert_eq!(wave(Format::Wav, &expected)?, expected);
    for end in 0..expected.len() {
        assert!(wave(Format::Wav, &expected[..end]).is_err());
    }
    for offset in 0..44 {
        let mut malformed = expected.clone();
        malformed[offset] ^= 1;
        assert!(wave(Format::Wav, &malformed).is_err());
    }
    for pcm in [vec![], vec![0], vec![0; MAX_PCM + 2]] {
        assert!(wave(Format::Pcm, &pcm).is_err());
    }
    let maximum = wave(Format::Pcm, &vec![0; MAX_PCM])?;
    assert_eq!(maximum.len(), MAX_PCM + 44);
    assert_eq!(wave(Format::Wav, &maximum)?, maximum);
    assert_eq!(Format::Pcm.media(), "audio/pcm");
    assert_eq!(Format::Wav.media(), "audio/wav");
    assert_eq!(Format::Pcm.name(), "pcm");
    assert_eq!(Format::Wav.name(), "wav");
    assert_eq!(Format::Pcm.limit(), MAX_PCM);
    assert_eq!(Format::Wav.limit(), MAX_PCM + 44);
    Ok(())
}

#[test]
fn readiness_requires_exact_true_flags_and_pinned_model() {
    let health = json!({"ready": true, "consistent_voice": true, "model": MODEL});
    assert!(ready(&health));
    for field in ["ready", "consistent_voice", "model"] {
        for bad in [Value::Null, json!(1), json!("true"), json!(false)] {
            let mut value = health.clone();
            value[field] = bad;
            assert!(!ready(&value));
        }
    }
}

fn session(quantization: &str, resident: u64, committed: u64) -> Value {
    json!({"Threads": 4, "Repetition": 1, "Quantization": quantization, "Status": "completed",
        "Memory": {"Status": "completed", "PeakWorkingSetBytes": resident, "PeakJobMemoryBytes": committed}})
}

#[test]
fn comparisons_keep_counter_domains_and_reject_failed_or_incomplete_pairs() -> crate::Result<()> {
    let pair = vec![session("Q8_0", 100, 20), session("Q4_K_M", 60, 50)];
    let result = comparisons(&pair, &[4], 1)?;
    assert_eq!(result[0]["Q8MinusQ4PeakWorkingSetBytes"], 40);
    assert_eq!(result[0]["Q8MinusQ4PeakJobMemoryBytes"], -30);
    assert!(comparisons(&pair[..1], &[4], 1).is_err());
    assert!(comparisons(&pair, &[4], 2).is_err());
    for field in ["Status", "Quantization", "Threads", "Repetition"] {
        let mut invalid = pair.clone();
        invalid[0][field] = Value::Null;
        assert!(comparisons(&invalid, &[4], 1).is_err());
    }
    for field in ["Status", "PeakWorkingSetBytes", "PeakJobMemoryBytes"] {
        for value in [
            Value::Null,
            json!(true),
            json!(-1),
            json!(1.5),
            json!(u64::MAX),
        ] {
            let mut invalid = pair.clone();
            invalid[0]["Memory"][field] = value;
            assert!(comparisons(&invalid, &[4], 1).is_err());
        }
    }
    let mut duplicate = pair;
    duplicate[1] = duplicate[0].clone();
    assert!(comparisons(&duplicate, &[4], 1).is_err());
    Ok(())
}

#[test]
fn evaluation_schema_rejects_unknown_fields_and_coerced_options() {
    let value =
        json!({"repositoryRoot": "C:/source", "dataDirectory": "C:/models", "repetitions": 2});
    assert!(serde_json::from_value::<Request>(value.clone()).is_ok());
    for (field, invalid) in [
        ("repetitions", json!(true)),
        ("repetitions", json!(2.5)),
        ("responseFormat", json!("mp3")),
        ("threads", json!([256])),
    ] {
        let mut bad = value.clone();
        bad[field] = invalid;
        assert!(serde_json::from_value::<Request>(bad).is_err());
    }
}
