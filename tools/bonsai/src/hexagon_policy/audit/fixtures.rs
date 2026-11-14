//! Source-bound fixture inventory usable from the checked maintenance executable.
use super::{
    Path, Result, TILING, Value, add, contract, contracts, json, ledgers, mul, pq2, read, require,
    tiling,
};
macro_rules! assert {
    ($condition:expr) => {
        require(
            $condition,
            concat!("Hexagon fixture failed at ", file!(), ":", line!()),
        )?
    };
}
macro_rules! assert_eq {
    ($left:expr, $right:expr) => {
        require(
            $left == $right,
            concat!("Hexagon equality fixture failed at ", file!(), ":", line!()),
        )?
    };
}
macro_rules! assert_ne {
    ($left:expr, $right:expr) => {
        require(
            $left != $right,
            concat!(
                "Hexagon inequality fixture failed at ",
                file!(),
                ":",
                line!()
            ),
        )?
    };
}
fn inventories(root: &Path) -> Result<(Value, Value)> {
    Ok((
        read(root, "config/swift.metadata.json")?,
        read(root, "config/swift.pq2-census.json")?,
    ))
}
fn block(scale: u16, byte: u8) -> Vec<u8> {
    let mut data = scale.to_le_bytes().to_vec();
    data.extend_from_slice(&[byte; 32]);
    data
}
pub(super) fn test_known_byte_fixture_splits_at_element_64_and_keeps_fourth_symbol(
    _root: &Path,
) -> Result<()> {
    let mut data = block(0x3c00, 228);
    data[18..].fill(27);
    let mut expected = vec![0, 60];
    expected.extend_from_slice(&[228; 16]);
    expected.extend_from_slice(&[0, 60]);
    expected.extend_from_slice(&[27; 16]);
    assert_eq!(pq2::bridge(&data, 128, 1)?, expected);
    let values = [-1.0_f32, 0.0, 1.0, 2.0]
        .repeat(16)
        .into_iter()
        .chain([2.0_f32, 1.0, 0.0, -1.0].repeat(16))
        .map(f32::to_bits)
        .collect::<Vec<_>>();
    assert_eq!(pq2::decoded(&data, 34)?, values);
    assert_eq!(pq2::decoded(&expected, 18)?, values);
    assert_eq!(
        pq2::payload(&data, 128, 1)?["RawCodeCounts"],
        json!([32, 32, 32, 32])
    );
    Ok(())
}

pub(super) fn test_every_packed_byte_and_lane_uses_native_little_endian_order(
    _root: &Path,
) -> Result<()> {
    for packed in 0..=255_u8 {
        let data = block(0x3800, packed);
        let actual = pq2::decoded(&pq2::bridge(&data, 128, 1)?, 18)?;
        let expected = (0..32)
            .flat_map(|_| {
                [0, 2, 4, 6]
                    .map(|shift| (f32::from(i16::from((packed >> shift) & 3) - 1) * 0.5).to_bits())
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
    Ok(())
}

pub(super) fn test_extreme_signed_subnormal_and_zero_scales_keep_raw_bits(
    _root: &Path,
) -> Result<()> {
    for bits in [
        0, 32768, 1, 32769, 1023, 33791, 1024, 33792, 15360, 48128, 31743, 64511,
    ] {
        assert_eq!(
            pq2::payload(&block(bits, 228), 128, 1)?["DecodedF32Bits"],
            "exact"
        );
    }
    Ok(())
}

pub(super) fn test_unaligned_multirow_and_odd_group_payload_is_exact(_root: &Path) -> Result<()> {
    for (columns, rows) in [(128_u64, 1_u64), (256, 2), (384, 3), (640, 1), (5120, 2)] {
        let blocks = columns / 128 * rows;
        let mut data = Vec::new();
        for index in 0..blocks {
            data.extend_from_slice(&block(
                0x3c00 + u16::try_from(index).map_err(|e| e.to_string())?,
                u8::try_from(index % 256).map_err(|e| e.to_string())?,
            ));
        }
        let report = pq2::payload(&data, columns, rows)?;
        assert_eq!(report["BridgedBytes"], blocks * 36);
        assert_eq!(report["DecodedF32Bits"], "exact");
    }
    Ok(())
}

pub(super) fn test_nonfinite_scale_is_rejected_even_for_all_zero_codes(_root: &Path) -> Result<()> {
    for bits in [31744, 64512, 31745, 32767, 64513, 65535] {
        assert!(pq2::bridge(&block(bits, 85), 128, 1).is_err());
    }

    Ok(())
}

pub(super) fn test_partial_rows_tail_and_oversized_payload_rejected(_root: &Path) -> Result<()> {
    for (columns, rows) in [(0, 1), (127, 1), (129, 1), (128, 0)] {
        assert!(pq2::bridge(&[0; 34], columns, rows).is_err());
    }
    for length in [0, 33, 35, 1_048_577] {
        assert!(pq2::bridge(&vec![0; length], 128, 1).is_err());
    }
    assert!(pq2::bridge(&[0; 34], u64::MAX - 127, u64::MAX).is_err());

    Ok(())
}

pub(super) fn test_inverse_rejects_unequal_scale_bits_including_signed_zero(
    _root: &Path,
) -> Result<()> {
    let mut data = pq2::bridge(&[0; 34], 128, 1)?;
    data[19] = 128;
    assert!(pq2::inverse(&data, 128, 1).is_err());
    assert!(pq2::inverse(&data[..35], 128, 1).is_err());
    Ok(())
}

pub(super) fn test_retained_inventory_storage_cost_is_exact(root: &Path) -> Result<()> {
    let (metadata, census) = inventories(root)?;
    let report = pq2::storage(&metadata, &census)?;
    for (field, value) in [
        ("SourcePackedBytes", 7_137_280_000_u64),
        ("BridgedPackedBytes", 7_557_120_000),
        ("AdditionalPackedBytes", 419_840_000),
        ("ArtifactBytesBeforeAlignmentChanges", 7_626_008_928),
    ] {
        assert_eq!(report[field], value);
    }
    assert_eq!(report["InventoryOnly"], true);
    Ok(())
}

pub(super) fn test_changed_identity_geometry_and_transform_fail_closed(root: &Path) -> Result<()> {
    let (mut metadata, census) = inventories(root)?;
    for (field, value) in [
        ("ArtifactSha256", json!("0".repeat(64))),
        ("Blocks", json!(1)),
        ("CodePlusTwo", json!(1)),
        ("NonfiniteScaleBlocks", json!(1)),
        ("TensorCount", json!(false)),
        ("SerializedBytesRead", json!(-1)),
        ("CodecCommit", json!("0".repeat(40))),
    ] {
        let mut changed = census.clone();
        changed[field] = value;
        assert!(pq2::storage(&metadata, &changed).is_err());
    }
    metadata["Scalars"]
        .as_object_mut()
        .ok_or("Missing scalar fixture.")?
        .remove("prism.hadamard.gdn_v_grouped");
    assert!(pq2::storage(&metadata, &census).is_err());
    Ok(())
}

pub(super) fn test_duplicate_identity_fields_are_rejected(_root: &Path) -> Result<()> {
    assert!(
        super::super::json::parse_bounded(
            "{\"ArtifactSha256\":\"a\",\"ArtifactSha256\":\"b\"}",
            1_048_576,
            64
        )
        .is_err()
    );

    Ok(())
}

pub(super) fn test_inventory_validation_rejects_ambiguous_types_and_unbounded_extents(
    root: &Path,
) -> Result<()> {
    let (metadata, census) = inventories(root)?;
    for (name, value) in [
        ("ModelRepository", json!(["ukisai", "model"])),
        ("ModelRevision", json!(["revision"])),
        ("ModelRepository", json!("")),
    ] {
        let (mut left, mut right) = (metadata.clone(), census.clone());
        left[name] = value.clone();
        right[name] = value;
        assert!(pq2::storage(&left, &right).is_err());
    }
    for name in ["Blocks", "Elements", "TensorCount", "SerializedBytesRead"] {
        let mut changed = census.clone();
        changed[name] = json!(18_446_744_073_709_551_616.0);
        assert!(pq2::storage(&metadata, &changed).is_err());
    }
    for literal in ["NaN", "Infinity", "-Infinity"] {
        assert!(
            super::super::json::parse_bounded(&format!("{{\"unused\":{literal}}}"), 1_048_576, 64)
                .is_err()
        );
    }
    assert!(
        super::super::json::parse_bounded(
            &format!("{{\"nested\":{}0{}}}", "[".repeat(5000), "]".repeat(5000)),
            1_048_576,
            64
        )
        .is_err()
    );
    Ok(())
}

pub(super) fn test_single_block_k128_plane_dots_are_exact_in_fp16_while_multi_block_overflows(
    _root: &Path,
) -> Result<()> {
    for value in -2048_i16..=2048 {
        assert_eq!(
            tiling::round_half(f32::from(value))?.to_bits(),
            f32::from(value).to_bits()
        );
    }
    assert_eq!(
        tiling::counterexamples()?["multiBlockFp16IntegerTruncation"]["fp16RoundedValueFor2049"],
        2048
    );
    Ok(())
}

pub(super) fn test_two_block_k256_distinct_scales_cannot_preserve_pinned_swift_fma(
    _root: &Path,
) -> Result<()> {
    let example = tiling::counterexamples()?["twoBlockK256DistinctScalesCounterexample"].clone();
    assert_eq!(example["pinnedSwiftPer128BlockFmaF32Bits"], "c1f90b01");
    assert_ne!(
        example["pinnedSwiftPer128BlockFmaF32Bits"],
        example["hmxBakedScaleK256Fp16StoreF32Bits"]
    );
    assert_eq!(example["matchesPinnedSwiftBitwise"], false);
    assert_eq!(example["unweightedScaleFactorable"], false);
    Ok(())
}

pub(super) fn test_dsp_plane_reduction_opcodes_are_not_supported_for_pinned_swift_bitplanes(
    _root: &Path,
) -> Result<()> {
    let reduction = contract(contracts::TILING)?["dspPlaneAndScaleReductionAudit"].clone();
    assert_eq!(
        reduction["hmxMinimumRowPredicate"]["constant"],
        "HTP_MM_HMX_MIN_NROWS = 4"
    );
    assert_eq!(
        reduction["sumRowsAxisMismatch"]["opcode"],
        "HTP_OP_SUM_ROWS (27)"
    );
    assert_eq!(
        reduction["missingSignedWeightedPlaneFmaOpcode"]["supportedBySignedV0100Package"],
        false
    );
    Ok(())
}

pub(super) fn test_hmx_2d_solver_selects_4352_row_chunks_with_2mib_vtcm_and_8_hvx_workers(
    _root: &Path,
) -> Result<()> {
    let legacy = tiling::chunks(17408, 65536, 1)?;
    assert_eq!(legacy["nChunk"], 128);
    assert_eq!(legacy["vtcmSizeBytes"], 57344);
    assert_eq!(legacy["chunksPer40OpProjection"], 5440);
    let optimized = tiling::chunks(17408, 2_097_152, 8)?;
    assert_eq!(optimized["nChunk"], 4352);
    assert_eq!(optimized["vtcmSizeBytes"], 1_576_960);
    assert_eq!(optimized["chunksPer40OpProjection"], 160);
    assert_eq!(optimized["tailWasteRows"], 0);
    for (rows, size) in [(32, 22528), (64, 34816)] {
        let small = tiling::chunks(rows, 2_097_152, 8)?;
        assert_eq!(small["nChunk"], rows);
        assert_eq!(small["vtcmSizeBytes"], size);
    }
    Ok(())
}

pub(super) fn test_solver_rejects_invalid_geometries_and_undersized_vtcm(
    _root: &Path,
) -> Result<()> {
    for (rows, budget, threads) in [
        (0, 2_097_152, 8),
        (33, 2_097_152, 8),
        (32, 16384, 8),
        (32, 2_097_152, 0),
        (32, 2_097_152, 9),
    ] {
        assert!(tiling::chunks(rows, budget, threads).is_err());
    }

    Ok(())
}

pub(super) fn test_retained_static_audit_json_matches_derived_report(root: &Path) -> Result<()> {
    assert_eq!(read(root, TILING)?, tiling::report(root)?);
    Ok(())
}

pub(super) fn payload_ledgers_preserve_exact_totals_and_native_fixture_inputs(
    root: &Path,
) -> Result<()> {
    let traffic = ledgers::traffic(root)?;
    assert_eq!(traffic["totals"]["operations"], 23256);
    assert_eq!(ledgers::budget(root)?["terminalMaximumOperations"], 24000);
    assert_eq!(ledgers::packed(root)?["Valid"], true);
    assert!(mul(u64::MAX, 2).is_err());
    assert!(add(u64::MAX, 1).is_err());
    Ok(())
}

pub(super) type Fixture = fn(&Path) -> Result<()>;
pub(super) const CASES: &[(&str, Fixture)] = &[
    (
        "test_known_byte_fixture_splits_at_element_64_and_keeps_fourth_symbol",
        test_known_byte_fixture_splits_at_element_64_and_keeps_fourth_symbol,
    ),
    (
        "test_every_packed_byte_and_lane_uses_native_little_endian_order",
        test_every_packed_byte_and_lane_uses_native_little_endian_order,
    ),
    (
        "test_extreme_signed_subnormal_and_zero_scales_keep_raw_bits",
        test_extreme_signed_subnormal_and_zero_scales_keep_raw_bits,
    ),
    (
        "test_unaligned_multirow_and_odd_group_payload_is_exact",
        test_unaligned_multirow_and_odd_group_payload_is_exact,
    ),
    (
        "test_nonfinite_scale_is_rejected_even_for_all_zero_codes",
        test_nonfinite_scale_is_rejected_even_for_all_zero_codes,
    ),
    (
        "test_partial_rows_tail_and_oversized_payload_rejected",
        test_partial_rows_tail_and_oversized_payload_rejected,
    ),
    (
        "test_inverse_rejects_unequal_scale_bits_including_signed_zero",
        test_inverse_rejects_unequal_scale_bits_including_signed_zero,
    ),
    (
        "test_retained_inventory_storage_cost_is_exact",
        test_retained_inventory_storage_cost_is_exact,
    ),
    (
        "test_changed_identity_geometry_and_transform_fail_closed",
        test_changed_identity_geometry_and_transform_fail_closed,
    ),
    (
        "test_duplicate_identity_fields_are_rejected",
        test_duplicate_identity_fields_are_rejected,
    ),
    (
        "test_inventory_validation_rejects_ambiguous_types_and_unbounded_extents",
        test_inventory_validation_rejects_ambiguous_types_and_unbounded_extents,
    ),
    (
        "test_single_block_k128_plane_dots_are_exact_in_fp16_while_multi_block_overflows",
        test_single_block_k128_plane_dots_are_exact_in_fp16_while_multi_block_overflows,
    ),
    (
        "test_two_block_k256_distinct_scales_cannot_preserve_pinned_swift_fma",
        test_two_block_k256_distinct_scales_cannot_preserve_pinned_swift_fma,
    ),
    (
        "test_dsp_plane_reduction_opcodes_are_not_supported_for_pinned_swift_bitplanes",
        test_dsp_plane_reduction_opcodes_are_not_supported_for_pinned_swift_bitplanes,
    ),
    (
        "test_hmx_2d_solver_selects_4352_row_chunks_with_2mib_vtcm_and_8_hvx_workers",
        test_hmx_2d_solver_selects_4352_row_chunks_with_2mib_vtcm_and_8_hvx_workers,
    ),
    (
        "test_solver_rejects_invalid_geometries_and_undersized_vtcm",
        test_solver_rejects_invalid_geometries_and_undersized_vtcm,
    ),
    (
        "test_retained_static_audit_json_matches_derived_report",
        test_retained_static_audit_json_matches_derived_report,
    ),
    (
        "payload_ledgers_preserve_exact_totals_and_native_fixture_inputs",
        payload_ledgers_preserve_exact_totals_and_native_fixture_inputs,
    ),
];
