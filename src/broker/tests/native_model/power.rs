//! Read-only power observations for future actual-model acceptance receipts.
#![expect(
    clippy::redundant_pub_crate,
    reason = "The private test helper shares snapshots only with its parent and sibling harnesses."
)]

use super::*;

pub(super) fn snapshot() -> Value {
    bonsai_windows_process::power_status().map_or_else(
        |_| bonsai_cpu_policy::power::unknown_snapshot(),
        |power| {
            bonsai_cpu_policy::power::observed_snapshot(
                power.ac_online,
                power.battery_saver,
                power.battery_percent,
            )
        },
    )
}

pub(super) fn begin(report: &mut Value) {
    report["powerBefore"] = snapshot();
    report["powerAfter"] = Value::Null;
    report["powerConditions"] =
        bonsai_cpu_policy::power::compare_endpoints(&report["powerBefore"], &report["powerAfter"]);
}

pub(super) fn finish(report: &mut Value) {
    report["powerAfter"] = snapshot();
    report["powerConditions"] =
        bonsai_cpu_policy::power::compare_endpoints(&report["powerBefore"], &report["powerAfter"]);
}
