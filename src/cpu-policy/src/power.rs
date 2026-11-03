//! Power-source conditions for retained trials, separate from correctness and energy claims.

use serde_json::{Value, json};

/// Formats a successful Windows power query, preserving unknown AC/charge sentinels.
#[must_use]
pub fn observed_snapshot(
    ac_online: Option<bool>,
    battery_saver: bool,
    battery_percent: Option<u8>,
) -> Value {
    json!({"status":"observed","source":"GetSystemPowerStatus",
        "ac_online":ac_online,"battery_saver":battery_saver,"battery_percent":battery_percent})
}

/// Formats an unavailable query without inferring AC state from other device evidence.
#[must_use]
pub fn unknown_snapshot() -> Value {
    json!({"status":"unknown","source":"GetSystemPowerStatus",
        "ac_online":null,"battery_saver":null,"battery_percent":null})
}

fn observed_bool(snapshot: &Value, field: &str) -> Option<bool> {
    (snapshot["status"] == "observed" && snapshot["source"] == "GetSystemPowerStatus")
        .then(|| snapshot[field].as_bool())
        .flatten()
}

/// Compares observed AC and battery-saver endpoints without claiming continuous stability.
///
/// Charge percentage is retained as context, not an eligibility condition. A missing
/// battery percentage can mean the device has no battery; it never supplies AC state.
#[must_use]
pub fn compare_endpoints(before: &Value, after: &Value) -> Value {
    let mut changed = Vec::new();
    let mut unknown = Vec::new();
    for field in ["ac_online", "battery_saver"] {
        match (observed_bool(before, field), observed_bool(after, field)) {
            (Some(first), Some(last)) if first != last => changed.push(field),
            (Some(_), Some(_)) => {}
            _ => unknown.push(field),
        }
    }
    let eligible = changed.is_empty() && unknown.is_empty();
    json!({"status":if !changed.is_empty() {"changed"} else if !unknown.is_empty() {"unknown"} else {"matched-known-endpoints"},
        "performanceComparisonEligible":eligible,"changed":changed,"unknown":unknown,
        "continuousPowerStabilityProven":false,
        "scope":"GetSystemPowerStatus endpoint observations only; equal endpoints do not rule out intervening AC or battery-saver changes. Charge level is context. No energy, thermal or power-policy qualification."})
}

/// Requires known matching AC/saver observations within and between all retained samples.
///
/// Each sample must supply `powerBefore` and `powerAfter`. Missing observations make
/// performance comparison ineligible while leaving the captured results available.
#[must_use]
pub fn compare_samples(samples: &[Value]) -> Value {
    let mut eligible = !samples.is_empty();
    let mut details = Vec::with_capacity(samples.len());
    for (index, sample) in samples.iter().enumerate() {
        let within = compare_endpoints(&sample["powerBefore"], &sample["powerAfter"]);
        let between = index.checked_sub(1).map(|previous| {
            compare_endpoints(&samples[previous]["powerAfter"], &sample["powerBefore"])
        });
        eligible &= within["performanceComparisonEligible"] == true
            && between
                .as_ref()
                .is_none_or(|comparison| comparison["performanceComparisonEligible"] == true);
        details.push(
            json!({"sampleIndex":index,"withinSample":within,"previousSampleBoundary":between}),
        );
    }
    json!({"status":if eligible {"matched-known-endpoints"} else {"ineligible"},
        "performanceComparisonEligible":eligible,"samples":details,
        "continuousPowerStabilityProven":false,
        "scope":"Known matching AC and battery-saver endpoints required within and between samples; equal endpoints do not prove continuous power stability. Correctness and captured answers remain independent."})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ac_and_battery_are_distinct_from_unknown_or_changed_power() {
        let ac = observed_snapshot(Some(true), false, Some(80));
        let battery = observed_snapshot(Some(false), false, Some(79));
        let unknown_ac = observed_snapshot(None, false, Some(80));
        for observation in [&ac, &battery] {
            let comparison = compare_endpoints(observation, observation);
            assert_eq!(comparison["performanceComparisonEligible"], true);
            assert_eq!(comparison["continuousPowerStabilityProven"], false);
        }
        assert_eq!(compare_endpoints(&ac, &battery)["status"], "changed");
        for observation in [&Value::Null, &unknown_snapshot(), &unknown_ac] {
            assert_eq!(compare_endpoints(&ac, observation)["status"], "unknown");
            assert_eq!(
                compare_endpoints(observation, observation)["performanceComparisonEligible"],
                false
            );
        }
        let charging = observed_snapshot(Some(true), false, Some(81));
        assert_eq!(
            compare_endpoints(&ac, &charging)["performanceComparisonEligible"],
            true
        );
        let saver = observed_snapshot(Some(true), true, Some(80));
        assert_eq!(compare_endpoints(&ac, &saver)["status"], "changed");
    }

    #[test]
    fn every_sample_and_inter_sample_boundary_requires_known_matching_power() {
        let ac = observed_snapshot(Some(true), false, Some(80));
        let battery = observed_snapshot(Some(false), false, Some(79));
        let sample = json!({"powerBefore":ac,"powerAfter":ac});
        assert_eq!(
            compare_samples(&[sample.clone(), sample.clone()])["performanceComparisonEligible"],
            true
        );
        for samples in [
            vec![],
            vec![Value::Null],
            vec![sample.clone(), Value::Null],
            vec![
                sample.clone(),
                json!({"powerBefore":battery,"powerAfter":battery}),
            ],
            vec![json!({"powerBefore":ac,"powerAfter":battery})],
        ] {
            assert_eq!(
                compare_samples(&samples)["performanceComparisonEligible"],
                false
            );
        }
        let mut wrong_source = sample;
        wrong_source["powerAfter"]["source"] = json!("assumed-from-historical-receipt");
        assert_eq!(
            compare_samples(&[wrong_source])["performanceComparisonEligible"],
            false
        );
    }
}
