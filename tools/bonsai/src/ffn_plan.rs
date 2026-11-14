//! Model-free packed-payload and factor accounting for explicit FFN omissions.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const WIDTH: u64 = 5_120;
const GROUP_WIDTH: u64 = 1_024;
const GROUPS: u64 = 17;
const BLOCK_COEFFICIENTS: u64 = 128;
const TARGET_BYTES: u64 = 512 * 1_024 * 1_024;

#[derive(Clone, Copy, Deserialize, Serialize)]
enum Representation {
    #[serde(rename = "PQ2_0")]
    Pq2,
    #[serde(rename = "current-repacked-PTQ1_0")]
    CurrentRepackedPtq1,
}

impl Representation {
    const fn block_bytes(self) -> u64 {
        match self {
            Self::Pq2 => 34,
            Self::CurrentRepackedPtq1 => 28,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Site {
    layer: u32,
    omitted_groups: Vec<u32>,
    recovery_rank: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    schema_version: u32,
    weight_representation: Representation,
    sites: Vec<Site>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Accounting {
    gross_omitted_packed_payload_bytes: u64,
    new_factor_payload_bytes: u64,
    net_packed_minus_factor_payload_bytes: u64,
    omitted_base_projection_multiply_accumulates_per_token: u64,
    new_factor_projection_multiply_accumulates_per_token: u64,
}

fn multiply(left: u64, right: u64) -> crate::Result<u64> {
    left.checked_mul(right)
        .ok_or_else(|| "FFN geometry multiplication overflowed.".into())
}

fn add(left: u64, right: u64) -> crate::Result<u64> {
    left.checked_add(right)
        .ok_or_else(|| "FFN geometry addition overflowed.".into())
}

impl Accounting {
    fn site(site: &Site, representation: Representation) -> crate::Result<Self> {
        let omitted_groups =
            u64::try_from(site.omitted_groups.len()).map_err(|error| error.to_string())?;
        let omitted_macs = multiply(multiply(3 * WIDTH, GROUP_WIDTH)?, omitted_groups)?;
        let factor_macs = multiply(2 * WIDTH, u64::from(site.recovery_rank))?;
        let gross_bytes = multiply(
            omitted_macs / BLOCK_COEFFICIENTS,
            representation.block_bytes(),
        )?;
        let factor_bytes = multiply(factor_macs, 4)?;
        let net_bytes = gross_bytes
            .checked_sub(factor_bytes)
            .ok_or("FFN factors exceed the omitted packed payload.")?;
        Ok(Self {
            gross_omitted_packed_payload_bytes: gross_bytes,
            new_factor_payload_bytes: factor_bytes,
            net_packed_minus_factor_payload_bytes: net_bytes,
            omitted_base_projection_multiply_accumulates_per_token: omitted_macs,
            new_factor_projection_multiply_accumulates_per_token: factor_macs,
        })
    }

    fn accumulate(&mut self, site: &Self) -> crate::Result<()> {
        self.gross_omitted_packed_payload_bytes = add(
            self.gross_omitted_packed_payload_bytes,
            site.gross_omitted_packed_payload_bytes,
        )?;
        self.new_factor_payload_bytes =
            add(self.new_factor_payload_bytes, site.new_factor_payload_bytes)?;
        self.net_packed_minus_factor_payload_bytes = add(
            self.net_packed_minus_factor_payload_bytes,
            site.net_packed_minus_factor_payload_bytes,
        )?;
        self.omitted_base_projection_multiply_accumulates_per_token = add(
            self.omitted_base_projection_multiply_accumulates_per_token,
            site.omitted_base_projection_multiply_accumulates_per_token,
        )?;
        self.new_factor_projection_multiply_accumulates_per_token = add(
            self.new_factor_projection_multiply_accumulates_per_token,
            site.new_factor_projection_multiply_accumulates_per_token,
        )?;
        Ok(())
    }
}

/// Accounts for one explicit heterogeneous whole-group FFN omission plan.
///
/// This reads no model, selects no sites and fits no factors. Passing the packed
/// payload envelope is only a necessary source-geometry screen for the material
/// RAM target; it establishes no actual process-memory or useful-answer benefit.
///
/// # Errors
/// Rejects unsupported schemas/representations, unknown fields, duplicate or
/// out-of-range sites, noncanonical whole groups, unsupported ranks and overflow.
pub fn run(value: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if request.schema_version != 1 || !(1..=64).contains(&request.sites.len()) {
        return Err("FFN plan requires schema 1 and 1..64 explicit sites.".into());
    }
    let mut layers = BTreeSet::new();
    let mut aggregate = Accounting::default();
    let mut sites = Vec::with_capacity(request.sites.len());
    for site in &request.sites {
        if site.layer >= 64
            || !layers.insert(site.layer)
            || !(1..=16).contains(&site.recovery_rank)
            || !(1..=16).contains(&site.omitted_groups.len())
            || site
                .omitted_groups
                .iter()
                .any(|&group| u64::from(group) >= GROUPS)
            || site
                .omitted_groups
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("FFN plan needs unique layers 0..63, ranks 1..16 and sorted distinct groups 0..16 with at least one retained.".into());
        }
        let accounting = Accounting::site(site, request.weight_representation)?;
        aggregate.accumulate(&accounting)?;
        sites.push(
            json!({"layer":site.layer,"omittedGroups":site.omitted_groups,
            "recoveryRank":site.recovery_rank,"accounting":accounting}),
        );
    }
    let reaches_target = aggregate.net_packed_minus_factor_payload_bytes >= TARGET_BYTES;
    Ok(
        json!({"schemaVersion":1,"kind":"experimental-structured-ffn-payload-plan-v1",
        "status":"geometry-only","weightRepresentation":request.weight_representation,
        "geometry":{"inputWidth":WIDTH,"outputWidth":WIDTH,"originalHiddenWidth":GROUP_WIDTH*GROUPS,
            "groupWidth":GROUP_WIDTH,"groupsPerSite":GROUPS,"packedBlockCoefficients":BLOCK_COEFFICIENTS,
            "packedBlockBytes":request.weight_representation.block_bytes(),"factorBytesPerCoefficient":4},
        "selectedSiteCount":sites.len(),"sites":sites,"aggregate":aggregate,
        "materialTarget":{"netPackedMinusFactorPayloadBytes":TARGET_BYTES,
            "payloadEnvelopeReachesTarget":reaches_target,
            "scope":"necessary packed-payload-minus-F32-factor envelope; actual serving private commit remains unmeasured"},
        "packedAndF32MacsHaveDifferentCosts":true,"bottleneckSelected":false,
        "sourceAuthenticated":false,"runtimeImplemented":false,"selectiveLoadingImplemented":false,
        "performanceQualified":false,"processMemoryQualified":false,"behaviorQualified":false,
        "actualProcessMemoryBenefitBytes":null,"usefulAnswerLatencyBenefit":null,"heldoutQualityBenefit":null,
        "excludedCosts":["norms and nonlinearities","standard adapters","signs and metadata",
            "alignment and allocator capacity","scratch and temporary copies","KV and recurrent state","kernel and dispatch costs"],
        "limits":"No model intake, fitting, execution, sensitivity ranking, installation or graph change. Selective payload loading and measured held-out quality, latency and process memory are independent gates."}),
    )
}

#[cfg(test)]
mod tests;
