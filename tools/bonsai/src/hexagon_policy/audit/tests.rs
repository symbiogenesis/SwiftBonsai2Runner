//! Native harness for the checked executable's source-bound fixtures.
use super::{PathBuf, Result};
#[test]
fn test_known_byte_fixture_splits_at_element_64_and_keeps_fourth_symbol() -> Result<()> {
    super::fixtures::test_known_byte_fixture_splits_at_element_64_and_keeps_fourth_symbol(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_every_packed_byte_and_lane_uses_native_little_endian_order() -> Result<()> {
    super::fixtures::test_every_packed_byte_and_lane_uses_native_little_endian_order(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_extreme_signed_subnormal_and_zero_scales_keep_raw_bits() -> Result<()> {
    super::fixtures::test_extreme_signed_subnormal_and_zero_scales_keep_raw_bits(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_unaligned_multirow_and_odd_group_payload_is_exact() -> Result<()> {
    super::fixtures::test_unaligned_multirow_and_odd_group_payload_is_exact(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_nonfinite_scale_is_rejected_even_for_all_zero_codes() -> Result<()> {
    super::fixtures::test_nonfinite_scale_is_rejected_even_for_all_zero_codes(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_partial_rows_tail_and_oversized_payload_rejected() -> Result<()> {
    super::fixtures::test_partial_rows_tail_and_oversized_payload_rejected(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_inverse_rejects_unequal_scale_bits_including_signed_zero() -> Result<()> {
    super::fixtures::test_inverse_rejects_unequal_scale_bits_including_signed_zero(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_retained_inventory_storage_cost_is_exact() -> Result<()> {
    super::fixtures::test_retained_inventory_storage_cost_is_exact(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_changed_identity_geometry_and_transform_fail_closed() -> Result<()> {
    super::fixtures::test_changed_identity_geometry_and_transform_fail_closed(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_duplicate_identity_fields_are_rejected() -> Result<()> {
    super::fixtures::test_duplicate_identity_fields_are_rejected(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_inventory_validation_rejects_ambiguous_types_and_unbounded_extents() -> Result<()> {
    super::fixtures::test_inventory_validation_rejects_ambiguous_types_and_unbounded_extents(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_single_block_k128_plane_dots_are_exact_in_fp16_while_multi_block_overflows() -> Result<()> {
    super::fixtures::test_single_block_k128_plane_dots_are_exact_in_fp16_while_multi_block_overflows(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_two_block_k256_distinct_scales_cannot_preserve_pinned_swift_fma() -> Result<()> {
    super::fixtures::test_two_block_k256_distinct_scales_cannot_preserve_pinned_swift_fma(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_dsp_plane_reduction_opcodes_are_not_supported_for_pinned_swift_bitplanes() -> Result<()> {
    super::fixtures::test_dsp_plane_reduction_opcodes_are_not_supported_for_pinned_swift_bitplanes(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_hmx_2d_solver_selects_4352_row_chunks_with_2mib_vtcm_and_8_hvx_workers() -> Result<()> {
    super::fixtures::test_hmx_2d_solver_selects_4352_row_chunks_with_2mib_vtcm_and_8_hvx_workers(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_solver_rejects_invalid_geometries_and_undersized_vtcm() -> Result<()> {
    super::fixtures::test_solver_rejects_invalid_geometries_and_undersized_vtcm(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn test_retained_static_audit_json_matches_derived_report() -> Result<()> {
    super::fixtures::test_retained_static_audit_json_matches_derived_report(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
#[test]
fn payload_ledgers_preserve_exact_totals_and_native_fixture_inputs() -> Result<()> {
    super::fixtures::payload_ledgers_preserve_exact_totals_and_native_fixture_inputs(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}
