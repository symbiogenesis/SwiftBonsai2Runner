//! Pure acceptance limits bound to the original job and optional owner clocks.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const TICKS_PER_SECOND: u64 = 10_000_000;
const OWNER_FIELDS: [&str; 5] = [
    "RunId",
    "Revision",
    "StartedTick",
    "StartedUtc",
    "WorkSeconds",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    job_started_utc: String,
    now_utc: String,
    now_tick: i64,
    tick_frequency: i64,
    run_id: String,
    revision: String,
    #[serde(default = "empty_owner")]
    owner_deadline: Value,
}

fn empty_owner() -> Value {
    serde_json::json!({})
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Budget {
    preparation_elapsed_seconds: f64,
    hard_limit_seconds: u16,
    case_budget_seconds: u16,
    owner_deadline_applied: bool,
    owner_elapsed_seconds: Option<f64>,
}

/// Calculates limits without reading the host clock, environment or model files.
///
/// Callers supply canonical seven-fraction UTC timestamps and the same monotonic
/// clock/frequency used by the inherited owner. The owner binding can only
/// shorten the independent job allowance; malformed or exhausted input rejects.
///
/// # Errors
/// Returns a diagnostic for invalid clock/binding fields or an exhausted budget.
pub fn run(request: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    let budget = calculate(&request)?;
    serde_json::to_value(budget).map_err(|error| error.to_string())
}

fn calculate(request: &Request) -> crate::Result<Budget> {
    if !canonical_decimal(&request.run_id, 20)
        || request.revision.len() != 40
        || !request
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Acceptance requires a canonical run ID and source revision.".into());
    }
    let job_started = utc_ticks(&request.job_started_utc)?;
    let now = utc_ticks(&request.now_utc)?;
    let setup_ticks = now
        .checked_sub(job_started)
        .ok_or("Acceptance setup time cannot be negative.")?;
    let now_tick = u64::try_from(request.now_tick)
        .map_err(|_| "Acceptance monotonic clocks must be nonnegative.")?;
    let frequency = u64::try_from(request.tick_frequency)
        .ok()
        .filter(|frequency| *frequency != 0)
        .ok_or("Acceptance monotonic clock frequency must be positive.")?;
    // Subtract the ceiling of elapsed time, preserving fractional setup costs
    // without allowing floating-point rounding to extend either deadline.
    let mut hard_limit = 250_i128.min(270 - i128::from(setup_ticks.div_ceil(TICKS_PER_SECOND)));
    let owner = request
        .owner_deadline
        .as_object()
        .ok_or("Owner deadline fields are invalid.")?;
    if owner
        .keys()
        .any(|field| !OWNER_FIELDS.contains(&field.as_str()))
    {
        return Err("Owner deadline fields are invalid.".into());
    }
    let supplied = OWNER_FIELDS
        .iter()
        .filter(|field| {
            owner
                .get(**field)
                .is_some_and(|value| !value.is_null() && value != "")
        })
        .count();
    let mut owner_elapsed = None;
    if supplied != 0 {
        if supplied != OWNER_FIELDS.len() {
            return Err("The complete owner deadline binding is required.".into());
        }
        let field = |name: &str| {
            owner
                .get(name)
                .and_then(Value::as_str)
                .ok_or_else(|| "Owner deadline fields must be strings.".to_owned())
        };
        if field("RunId")? != request.run_id
            || field("Revision")? != request.revision
            || field("WorkSeconds")? != "210"
        {
            return Err("The owner deadline run, revision or fixed allowance differs.".into());
        }
        let started_tick = field("StartedTick")?;
        if !canonical_decimal(started_tick, 19) {
            return Err("The original owner monotonic start is invalid or in the future.".into());
        }
        let started_tick = started_tick
            .parse::<i64>()
            .ok()
            .and_then(|tick| u64::try_from(tick).ok())
            .filter(|tick| *tick <= now_tick)
            .ok_or("The original owner monotonic start is invalid or in the future.")?;
        if utc_ticks(field("StartedUtc")?)? > now {
            return Err("The owner start requires a nonfuture round-trip UTC timestamp.".into());
        }
        let elapsed_ticks = now_tick - started_tick;
        // Fifteen seconds remain for process completion, collection and runner
        // exit before the independent original 210-second owner work cutoff.
        hard_limit = hard_limit.min(195 - i128::from(elapsed_ticks.div_ceil(frequency)));
        owner_elapsed = Some(seconds(elapsed_ticks, frequency)?);
    }
    let case_budget = 240_i128.min(hard_limit - 10);
    if case_budget < 1 {
        return Err(
            "Preparation exhausted the acceptance allowance; qualification remains pending.".into(),
        );
    }
    Ok(Budget {
        preparation_elapsed_seconds: seconds(setup_ticks, TICKS_PER_SECOND)?,
        hard_limit_seconds: u16::try_from(hard_limit).map_err(|error| error.to_string())?,
        case_budget_seconds: u16::try_from(case_budget).map_err(|error| error.to_string())?,
        owner_deadline_applied: supplied == OWNER_FIELDS.len(),
        owner_elapsed_seconds: owner_elapsed,
    })
}

fn seconds(ticks: u64, frequency: u64) -> crate::Result<f64> {
    let nanos = (u128::from(ticks % frequency) * 1_000_000_000) / u128::from(frequency);
    Ok(Duration::new(
        ticks / frequency,
        u32::try_from(nanos).map_err(|error| error.to_string())?,
    )
    .as_secs_f64())
}

fn canonical_decimal(value: &str, maximum_digits: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_digits
        && value.as_bytes()[0] != b'0'
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn timestamp_part(value: &str, start: usize, end: usize) -> crate::Result<u64> {
    value
        .get(start..end)
        .filter(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|part| part.parse().ok())
        .ok_or_else(|| "Acceptance clocks require canonical round-trip UTC timestamps.".into())
}

fn utc_ticks(value: &str) -> crate::Result<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 28
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[27] != b'Z'
    {
        return Err("Acceptance clocks require canonical round-trip UTC timestamps.".into());
    }
    let year = timestamp_part(value, 0, 4)?;
    let month = timestamp_part(value, 5, 7)?;
    let day = timestamp_part(value, 8, 10)?;
    let hour = timestamp_part(value, 11, 13)?;
    let minute = timestamp_part(value, 14, 16)?;
    let second = timestamp_part(value, 17, 19)?;
    let fraction = timestamp_part(value, 20, 27)?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let month_days = [
        31_u64,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let month_index = month
        .checked_sub(1)
        .and_then(|month| usize::try_from(month).ok())
        .filter(|month| *month < month_days.len())
        .ok_or("Acceptance UTC month is invalid.")?;
    if year == 0
        || day == 0
        || day > month_days[month_index]
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err("Acceptance UTC date or time is invalid.".into());
    }
    let previous_year = year - 1;
    let days = previous_year * 365 + previous_year / 4 - previous_year / 100
        + previous_year / 400
        + month_days[..month_index].iter().sum::<u64>()
        + day
        - 1;
    Ok((((days * 24 + hour) * 60 + minute) * 60 + second) * TICKS_PER_SECOND + fraction)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{run, utc_ticks};

    fn request() -> Value {
        json!({
            "jobStartedUtc": "2026-10-31T00:00:30.0000000Z",
            "nowUtc": "2026-10-31T00:00:50.0000000Z",
            "nowTick": 500_000,
            "tickFrequency": 1000,
            "runId": "12345",
            "revision": "a".repeat(40)
        })
    }

    fn owner() -> Value {
        json!({
            "RunId": "12345", "Revision": "a".repeat(40), "StartedTick": "450000",
            "StartedUtc": "2026-10-31T00:00:00.0000000Z", "WorkSeconds": "210"
        })
    }

    #[test]
    fn absent_owner_preserves_independent_job_allowance() -> crate::Result<()> {
        let mut input = request();
        for deadline in [
            json!({}),
            json!({"RunId": null, "Revision": "", "StartedTick": null, "StartedUtc": "", "WorkSeconds": null}),
        ] {
            input["ownerDeadline"] = deadline;
            let report = run(input.clone())?;
            assert_eq!(report["HardLimitSeconds"], 250);
            assert_eq!(report["CaseBudgetSeconds"], 240);
            assert_eq!(report["PreparationElapsedSeconds"].as_f64(), Some(20.0));
            assert_eq!(report["OwnerDeadlineApplied"], false);
            assert!(report["OwnerElapsedSeconds"].is_null());
        }
        input["jobStartedUtc"] = json!("2026-10-30T23:56:40.0000000Z");
        let report = run(input)?;
        assert_eq!(report["HardLimitSeconds"], 20);
        assert_eq!(report["CaseBudgetSeconds"], 10);
        Ok(())
    }

    #[test]
    fn original_owner_binding_only_shortens_both_deadlines() -> crate::Result<()> {
        let mut input = request();
        input["ownerDeadline"] = owner();
        let report = run(input.clone())?;
        assert_eq!(report["HardLimitSeconds"], 145);
        assert_eq!(report["CaseBudgetSeconds"], 135);
        assert_eq!(report["OwnerElapsedSeconds"].as_f64(), Some(50.0));
        assert_eq!(report["OwnerDeadlineApplied"], true);
        input["ownerDeadline"]["StartedTick"] = json!("350000");
        let report = run(input.clone())?;
        assert_eq!(report["HardLimitSeconds"], 45);
        assert_eq!(report["CaseBudgetSeconds"], 35);
        input["ownerDeadline"] = owner();
        input["jobStartedUtc"] = json!("2026-10-30T23:56:40.0000000Z");
        let report = run(input)?;
        assert_eq!(report["HardLimitSeconds"], 20);
        assert_eq!(report["CaseBudgetSeconds"], 10);
        Ok(())
    }

    #[test]
    fn fractional_setup_and_owner_costs_never_round_up_available_time() -> crate::Result<()> {
        let mut input = request();
        input["ownerDeadline"] = owner();
        input["ownerDeadline"]["StartedTick"] = json!("316000");
        let report = run(input.clone())?;
        assert_eq!(report["HardLimitSeconds"], 11);
        assert_eq!(report["CaseBudgetSeconds"], 1);
        input["ownerDeadline"]["StartedTick"] = json!("315999");
        assert!(run(input).is_err());
        let mut input = request();
        input["jobStartedUtc"] = json!("2026-10-30T23:56:31.0000000Z");
        assert_eq!(run(input.clone())?["CaseBudgetSeconds"], 1);
        input["jobStartedUtc"] = json!("2026-10-30T23:56:30.9999999Z");
        assert!(run(input).is_err());
        Ok(())
    }

    #[test]
    fn incomplete_mismatched_and_malformed_owner_bindings_reject() -> crate::Result<()> {
        for field in super::OWNER_FIELDS {
            let mut deadline = owner();
            deadline
                .as_object_mut()
                .ok_or("Expected owner fixture object.")?
                .remove(field);
            let mut input = request();
            input["ownerDeadline"] = deadline;
            assert!(run(input).is_err(), "Accepted missing field {field}");
        }
        let invalid = [
            ("RunId", json!("54321")),
            ("Revision", json!("b".repeat(40))),
            ("WorkSeconds", json!("211")),
            ("WorkSeconds", json!("0210")),
            ("StartedTick", json!("500001")),
            ("StartedTick", json!("0")),
            ("StartedTick", json!("-1")),
            ("StartedTick", json!("+450000")),
            ("StartedTick", json!("not-a-tick")),
            ("StartedTick", json!("9223372036854775808")),
            ("StartedTick", json!("300000")),
            ("StartedTick", json!(450_000)),
            ("StartedUtc", json!("2026-10-31T00:00:00.0000000+00:00")),
            ("StartedUtc", json!("2026-10-31T00:00:00.0000000")),
            ("StartedUtc", json!("2026-10-31T00:00:00.0000000-05:00")),
            ("StartedUtc", json!("not-utc")),
            ("StartedUtc", json!("2026-10-31T00:01:00.0000000Z")),
            ("Extra", json!("210")),
        ];
        for (field, value) in invalid {
            let mut input = request();
            input["ownerDeadline"] = owner();
            input["ownerDeadline"][field] = value;
            assert!(run(input).is_err(), "Accepted invalid owner field {field}");
        }
        for owner in [Value::Null, json!([]), json!(false), json!("binding")] {
            let mut input = request();
            input["ownerDeadline"] = owner;
            assert!(run(input).is_err());
        }
        Ok(())
    }

    #[test]
    fn invalid_job_clocks_identity_and_exhausted_limits_reject() {
        for (field, value) in [
            ("jobStartedUtc", json!("2026-10-30T23:56:20.0000000Z")),
            ("jobStartedUtc", json!("2026-10-31T00:01:00.0000000Z")),
            ("tickFrequency", json!(0)),
            ("tickFrequency", json!(-1)),
            ("tickFrequency", json!(1.5)),
            ("nowTick", json!(-1)),
            ("nowTick", json!("500000")),
            ("nowUtc", json!("2026-10-31T00:00:50.0000000")),
            ("nowUtc", json!("2026-10-31T00:00:50.0000000+00:00")),
            ("runId", json!("012345")),
            ("runId", json!("1".repeat(21))),
            ("revision", json!("A".repeat(40))),
            ("revision", json!("a".repeat(39))),
            ("extra", json!(true)),
        ] {
            let mut input = request();
            input[field] = value;
            assert!(run(input).is_err(), "Accepted invalid job field {field}");
        }
    }

    #[test]
    fn canonical_utc_parser_preserves_dotnet_calendar_and_tick_boundaries() -> crate::Result<()> {
        assert_eq!(utc_ticks("0001-01-01T00:00:00.0000000Z")?, 0);
        assert_eq!(
            utc_ticks("9999-12-31T23:59:59.9999999Z")?,
            3_155_378_975_999_999_999
        );
        assert_eq!(
            utc_ticks("2024-03-01T00:00:00.0000000Z")? - utc_ticks("2024-02-28T00:00:00.0000000Z")?,
            2 * 86400 * super::TICKS_PER_SECOND
        );
        for value in [
            "0000-01-01T00:00:00.0000000Z",
            "1900-02-29T00:00:00.0000000Z",
            "2026-02-29T00:00:00.0000000Z",
            "2026-04-31T00:00:00.0000000Z",
            "2026-00-31T00:00:00.0000000Z",
            "2026-13-31T00:00:00.0000000Z",
            "2026-01-00T00:00:00.0000000Z",
            "2026-01-31T24:00:00.0000000Z",
            "2026-01-31T00:60:00.0000000Z",
            "2026-01-31T00:00:60.0000000Z",
            "2026-01-31T00:00:00.000000Z",
            "2026-01-31T00:00:00.0000000z",
            "２０２６-01-31T00:00:00.0000000Z",
        ] {
            assert!(
                utc_ticks(value).is_err(),
                "Accepted invalid timestamp {value}"
            );
        }
        Ok(())
    }
}
