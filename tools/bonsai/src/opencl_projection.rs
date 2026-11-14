//! Opt-in, fixed-shape original-PQ2 `OpenCL` pilot; never a serving backend.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Value, json};

const COLUMNS: usize = 5120;
const ROWS: usize = 17_408;
const BLOCKS: usize = COLUMNS / 128;
const WEIGHT_BYTES: usize = ROWS * BLOCKS * 34;
#[cfg(windows)]
const Q8K_BYTES: usize = COLUMNS / 256 * 292;
const DOT_BYTES: usize = ROWS * BLOCKS * 4;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    path: PathBuf,
    sha256: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeviceSelection {
    platform_name: String,
    device_name: String,
    driver_version: String,
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "The driver selection is executed only on native Windows ARM64."
        )
    )]
    vendor_id: u32,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    weights: Input,
    q8k: Input,
    kernel_sha256: String,
    device: DeviceSelection,
    output_directory: PathBuf,
}

impl Request {
    fn parse(value: &Value) -> crate::Result<Self> {
        let request: Self =
            serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
        for path in [
            &request.weights.path,
            &request.q8k.path,
            &request.output_directory,
        ] {
            bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())?;
        }
        for hash in [
            &request.weights.sha256,
            &request.q8k.sha256,
            &request.kernel_sha256,
        ] {
            if !crate::identity::is_lower_hex(hash, 64) {
                return Err("OpenCL pilot requires exact lowercase SHA-256 identities.".into());
            }
        }
        for text in [
            &request.device.platform_name,
            &request.device.device_name,
            &request.device.driver_version,
        ] {
            if text.is_empty() || text.len() > 1024 || text.chars().any(char::is_control) {
                return Err("Invalid exact OpenCL device selection.".into());
            }
        }
        Ok(request)
    }
}

fn base() -> Value {
    let unknown_power = json!({"ac_online":null,"battery_saver":null,"battery_percent":null});
    json!({"schema_version":1, "kind":"runner-opencl-pq2-projection-pilot",
        "status":"unknown", "scope":"standalone-packed-projection",
        "columns":COLUMNS, "rows":ROWS, "source_tensor_shape":"blk.0.ffn_gate.weight",
        "model_binding_verified":false, "adapters_evaluated":false,
        "full_state_evaluated":false, "backend_ready":false, "qualification":"unqualified",
        "performance_comparison_eligible":false,
        "arithmetic":"original-pq2-q8k-per-128-integer-dot-cpu-ordered-fma",
        "explicit_device_buffer_bytes":WEIGHT_BYTES+COLUMNS+DOT_BYTES,
        "driver_compiler_memory_budget_bytes":null,
        "aggregate_power_before":unknown_power, "aggregate_power_after":unknown_power,
        "cpu_power_before":unknown_power, "cpu_power_after":unknown_power,
        "gpu_power_before":unknown_power, "gpu_power_after":unknown_power,
        "endpoint_power_does_not_prove_stability":true})
}

/// Run the fixed original-PQ2 projection in a deadline-owned native ARM64 child.
///
/// Requires exact raw input, kernel and device/driver identities and a new
/// absolute output directory. The child inherits the unchanged headless worker
/// mitigations, a 1 GiB one-process Job and the existing ten-second work bound.
/// Cold compiler startup can exhaust this bound and is never retried with weaker
/// policy. This command does not load a model or qualify an application profile.
///
/// # Errors
/// Rejects malformed input, altered identities, existing output directories and
/// failures to publish bounded supervision evidence.
pub fn run(value: &Value) -> crate::Result<Value> {
    let request = Request::parse(value)?;
    #[cfg(windows)]
    {
        windows::run(value, &request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Ok(base())
    }
}

/// Internal child entry point; use `opencl-projection` for deadline ownership.
///
/// Native driver calls have no intrinsic timeout. Raw device integer outputs are
/// retained for independent replay even when parity fails; final projection bytes
/// are published only after every original block agrees with the scalar control.
///
/// # Errors
/// Rejects changed input/kernel/device identity, invalid prepared `Q8_K` records,
/// nonfinite scales/reductions, failed native execution and output publication.
pub fn worker(value: &Value) -> crate::Result<Value> {
    let request = Request::parse(value)?;
    #[cfg(windows)]
    {
        windows::worker(&request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Ok(base())
    }
}

#[cfg(any(windows, test))]
struct Prepared {
    codes: Vec<u8>,
    scales: Vec<f32>,
}

#[cfg(any(windows, test))]
fn prepared(q8k: &[u8], columns: usize) -> crate::Result<Prepared> {
    if columns == 0 || !columns.is_multiple_of(256) || q8k.len() != columns / 256 * 292 {
        return Err("Incomplete original Q8_K records.".into());
    }
    let mut codes = Vec::with_capacity(columns);
    let mut scales = Vec::with_capacity(columns / 256);
    for block in q8k.as_chunks::<292>().0 {
        let scale = f32::from_le_bytes(block[..4].try_into().map_err(|_| "Invalid Q8_K scale.")?);
        if !scale.is_finite() {
            return Err("Nonfinite original Q8_K scale.".into());
        }
        for (values, sum) in block[4..260]
            .as_chunks::<16>()
            .0
            .iter()
            .zip(block[260..].as_chunks::<2>().0)
        {
            let sum = i16::from_le_bytes(*sum);
            let expected: i16 = values
                .iter()
                .map(|&value| i16::from(i8::from_ne_bytes([value])))
                .sum();
            if sum != expected {
                return Err("Original Q8_K block sums disagree with its signed codes.".into());
            }
        }
        scales.push(scale);
        codes.extend_from_slice(&block[4..260]);
    }
    Ok(Prepared { codes, scales })
}

#[cfg(any(windows, test))]
fn half(bytes: &[u8]) -> crate::Result<f32> {
    let bits = u16::from_le_bytes(bytes.try_into().map_err(|_| "Invalid PQ2 half scale.")?);
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = u32::from((bits >> 10) & 31);
    let fraction = bits & 1023;
    match exponent {
        31 => Err("Nonfinite original PQ2 scale.".into()),
        0 => {
            let value = f32::from(fraction) * (1.0 / 16_777_216.0);
            Ok(f32::from_bits(value.to_bits() | sign))
        }
        _ => Ok(f32::from_bits(
            sign | ((exponent + 112) << 23) | (u32::from(fraction) << 13),
        )),
    }
}

#[cfg(any(windows, test))]
fn scalar(
    weights: &[u8],
    input: &Prepared,
    columns: usize,
    rows: usize,
) -> crate::Result<(Vec<i32>, Vec<f32>)> {
    if columns == 0
        || !columns.is_multiple_of(256)
        || rows == 0
        || input.codes.len() != columns
        || input.scales.len() != columns / 256
        || columns
            .checked_div(128)
            .and_then(|blocks| blocks.checked_mul(rows))
            .and_then(|blocks| blocks.checked_mul(34))
            != Some(weights.len())
    {
        return Err("Invalid complete PQ2/Q8_K projection geometry.".into());
    }
    let blocks = columns / 128;
    let mut dots = Vec::with_capacity(rows * blocks);
    let mut output = Vec::with_capacity(rows);
    for row in weights.chunks_exact(blocks * 34) {
        let mut sum = 0.0_f32;
        for (index, (block, codes)) in row
            .as_chunks::<34>()
            .0
            .iter()
            .zip(input.codes.as_chunks::<128>().0)
            .enumerate()
        {
            let mut dot = 0_i32;
            // Independent scalar byte traversal: four adjacent symbols from
            // each original packed byte, including code 3 and signed -128.
            for (&packed, values) in block[2..].iter().zip(codes.as_chunks::<4>().0) {
                for (shift, &value) in [0, 2, 4, 6].into_iter().zip(values) {
                    dot += (i32::from((packed >> shift) & 3) - 1)
                        * i32::from(i8::from_ne_bytes([value]));
                }
            }
            dots.push(dot);
            let scale = half(&block[..2])? * input.scales[index / 2];
            // The integer lies in [-32768,32768] and converts exactly to F32.
            let integer = f32::from(
                i16::try_from(dot)
                    .map_err(|_| "Original block integer dot exceeds its proven bound.")?,
            );
            sum = scale.mul_add(integer, sum);
            if !sum.is_finite() {
                return Err("Nonfinite original ordered-FMA projection.".into());
            }
        }
        output.push(sum);
    }
    Ok((dots, output))
}

#[cfg(windows)]
mod windows {
    use super::{
        COLUMNS, DOT_BYTES, Input, Q8K_BYTES, ROWS, Request, Value, WEIGHT_BYTES, base, json,
        prepared, scalar,
    };
    use bonsai_windows_process::{OpenClProjectionSelection, ProcessSpec};
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read as _, Write as _};
    use std::path::Path;

    fn power() -> Value {
        bonsai_windows_process::power_status().map_or_else(
            |_| json!({"ac_online":null,"battery_saver":null,"battery_percent":null}),
            |value| json!({"ac_online":value.ac_online,"battery_saver":value.battery_saver,"battery_percent":value.battery_percent}),
        )
    }

    fn read(input: &Input, expected: usize) -> crate::Result<Vec<u8>> {
        let file = File::open(&input.path).map_err(|error| error.to_string())?;
        let mut bytes = Vec::with_capacity(expected);
        file.take(u64::try_from(expected).map_err(|error| error.to_string())? + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() != expected || crate::identity::sha256(&bytes) != input.sha256 {
            return Err("OpenCL pilot raw input size or SHA-256 identity changed.".into());
        }
        Ok(bytes)
    }

    fn publish(path: &Path, bytes: &[u8]) -> crate::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())
    }

    fn output_identity(path: &Path, bytes: &[u8]) -> Value {
        json!({"file":path.file_name().and_then(std::ffi::OsStr::to_str),"bytes":bytes.len(),"sha256":crate::identity::sha256(bytes)})
    }

    fn checkpoint(directory: &Path, report: &Value) -> crate::Result<()> {
        let bytes = serde_json::to_vec(report).map_err(|error| error.to_string())?;
        if bytes.len() > 64 * 1024 {
            return Err("OpenCL progress exceeds its fixed bound.".into());
        }
        let stage = directory.join("progress-next.json");
        publish(&stage, &bytes)?;
        // Same-directory rename replaces the previous complete checkpoint
        // atomically. A killed or failed publication leaves its prior snapshot.
        fs::rename(&stage, directory.join("progress.json")).map_err(|error| error.to_string())
    }

    fn recover_power(directory: &Path, report: &mut Value) -> crate::Result<bool> {
        let path = directory.join("progress.json");
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(64 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 64 * 1024 {
            return Err("OpenCL progress exceeds its fixed bound.".into());
        }
        let progress: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if progress["kind"] != "runner-opencl-pq2-projection-pilot"
            || progress["schema_version"] != 1
        {
            return Err("OpenCL progress identity is invalid.".into());
        }
        for key in [
            "cpu_power_before",
            "cpu_power_after",
            "gpu_power_before",
            "gpu_power_after",
        ] {
            let power = &progress[key];
            if !power.as_object().is_some_and(|object| object.len() == 3)
                || !["ac_online", "battery_saver"]
                    .into_iter()
                    .all(|name| power[name].is_boolean() || power[name].is_null())
                || !(power["battery_percent"].is_null()
                    || power["battery_percent"]
                        .as_u64()
                        .is_some_and(|percent| percent <= 100))
                || !power.as_object().is_some_and(|object| {
                    ["ac_online", "battery_saver", "battery_percent"]
                        .into_iter()
                        .all(|key| object.contains_key(key))
                })
            {
                return Err("OpenCL progress contains invalid power observations.".into());
            }
        }
        for key in [
            "cpu_power_before",
            "cpu_power_after",
            "gpu_power_before",
            "gpu_power_after",
        ] {
            if progress[key].is_object() {
                report[key] = progress[key].clone();
            }
        }
        Ok(true)
    }

    pub(super) fn worker(request: &Request) -> crate::Result<Value> {
        let kernel = bonsai_windows_process::opencl_pq2_kernel_source();
        if crate::identity::sha256(kernel.as_bytes()) != request.kernel_sha256 {
            return Err(
                "OpenCL pilot compiled kernel source differs from its reviewed identity.".into(),
            );
        }
        let weights = read(&request.weights, WEIGHT_BYTES)?;
        let q8k = read(&request.q8k, Q8K_BYTES)?;
        let input = prepared(&q8k, COLUMNS)?;
        let mut report = base();
        report["kernel_sha256"] = json!(request.kernel_sha256);
        report["weights_sha256"] = json!(request.weights.sha256);
        report["q8k_sha256"] = json!(request.q8k.sha256);
        report["cpu_power_before"] = power();
        checkpoint(&request.output_directory, &report)?;
        let cpu = scalar(&weights, &input, COLUMNS, ROWS);
        report["cpu_power_after"] = power();
        checkpoint(&request.output_directory, &report)?;
        let (expected, projection) = match cpu {
            Ok(result) => result,
            Err(error) => {
                report["error"] = json!(error);
                return Ok(report);
            }
        };
        let selection = OpenClProjectionSelection {
            platform_name: request.device.platform_name.clone(),
            device_name: request.device.device_name.clone(),
            driver_version: request.device.driver_version.clone(),
            vendor_id: request.device.vendor_id,
        };
        report["gpu_power_before"] = power();
        checkpoint(&request.output_directory, &report)?;
        let device =
            bonsai_windows_process::opencl_pq2_projection(&selection, &weights, &input.codes);
        report["gpu_power_after"] = power();
        checkpoint(&request.output_directory, &report)?;
        let device = match device {
            Ok(device) => device,
            Err(error) => {
                report["error"] = json!(error.to_string());
                return Ok(report);
            }
        };
        if device.integer_dots.len() != expected.len() {
            return Err("OpenCL pilot returned an incomplete projection.".into());
        }
        let mut bytes = Vec::with_capacity(DOT_BYTES);
        for value in &device.integer_dots {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let dots = request.output_directory.join("device-dots.i32le");
        publish(&dots, &bytes)?;
        let mismatch = expected
            .iter()
            .zip(&device.integer_dots)
            .enumerate()
            .find(|(_, (expected, actual))| expected != actual);
        report["status"] = json!(if mismatch.is_none() {
            "exact-integer-parity"
        } else {
            "integer-parity-failed"
        });
        report["integer_parity"] = json!(mismatch.is_none());
        report["compared_original_blocks"] = json!(expected.len());
        report["first_mismatch"] = mismatch.map_or(Value::Null, |(index, (expected, actual))| json!({"index":index,"expected":expected,"actual":actual}));
        report["device_dots"] = output_identity(&dots, &bytes);
        report["native_cleanup_codes"] = json!(device.cleanup_codes);
        report["native_cleanup_verified"] =
            json!(device.cleanup_codes.iter().all(|code| *code == 0));
        report["selected_device"] = json!({"platform_name":selection.platform_name,"name":device.device.name,"driver_version":device.device.driver_version,"vendor_id":device.device.vendor_id});
        let loader = File::open(&device.loader_path).map_err(|error| error.to_string())?;
        let mut loader_bytes = Vec::new();
        loader
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut loader_bytes)
            .map_err(|error| error.to_string())?;
        if loader_bytes.is_empty() || loader_bytes.len() > 16 * 1024 * 1024 {
            return Err("OpenCL loader observation exceeds its bound.".into());
        }
        report["loader"] = json!({"file":"OpenCL.dll","bytes":loader_bytes.len(),"sha256":crate::identity::sha256(&loader_bytes),"signature_verified":false,"executed_bytes_verified":false,"dependencies_verified":false});
        if mismatch.is_none() {
            let mut bytes = Vec::with_capacity(ROWS * 4);
            for value in projection {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            let output = request.output_directory.join("projection.f32le");
            publish(&output, &bytes)?;
            report["projection"] = output_identity(&output, &bytes);
        }
        Ok(report)
    }

    pub(super) fn run(value: &Value, request: &Request) -> crate::Result<Value> {
        let before = power();
        fs::create_dir(&request.output_directory).map_err(|error| error.to_string())?;
        let input = request.output_directory.join("request.json");
        publish(
            &input,
            &serde_json::to_vec(value).map_err(|error| error.to_string())?,
        )?;
        let spec = ProcessSpec {
            executable: std::env::current_exe().map_err(|error| error.to_string())?,
            arguments: vec![
                "opencl-projection-worker".into(),
                "--request".into(),
                input.into_os_string(),
            ],
            working_directory: request.output_directory.clone(),
            stdout: request.output_directory.join("stdout.json"),
            stderr: request.output_directory.join("stderr.txt"),
            memory_limit_bytes: 1024 * 1024 * 1024,
            active_process_limit: 1,
        };
        let result =
            crate::runtime_probe::run(&spec, 1024 * 1024, "OpenCL PQ2 projection", || Ok(()));
        let mut report = match result {
            Ok(output) => {
                let parsed = serde_json::from_str::<Value>(&output.stdout);
                let valid = parsed.as_ref().is_ok_and(|report| {
                    report["kind"] == "runner-opencl-pq2-projection-pilot"
                        && report["schema_version"] == 1
                        && report["backend_ready"] == false
                        && report["qualification"] == "unqualified"
                        && report["performance_comparison_eligible"] == false
                });
                let mut report = match parsed {
                    Ok(report) if valid => report,
                    _ => {
                        let mut report = base();
                        report["error"] = json!("OpenCL worker returned an invalid pilot report.");
                        report
                    }
                };
                report["owned_job_cleanup"] = json!("confirmed");
                if output.stderr.trim().is_empty() {
                    fs::remove_file(&spec.stderr).map_err(|error| error.to_string())?;
                }
                if valid || output.stdout.is_empty() {
                    fs::remove_file(&spec.stdout).map_err(|error| error.to_string())?;
                }
                report
            }
            Err(error) => {
                let mut report = base();
                report["error"] = json!(error);
                report["owned_job_cleanup"] = json!("unknown-on-supervision-error");
                for path in [&spec.stdout, &spec.stderr] {
                    if fs::metadata(path).is_ok_and(|metadata| metadata.len() == 0) {
                        fs::remove_file(path).map_err(|error| error.to_string())?;
                    }
                }
                report
            }
        };
        let recovered = match recover_power(&request.output_directory, &mut report) {
            Ok(recovered) => recovered,
            Err(error) => {
                report["power_checkpoint_error"] = json!(error);
                false
            }
        };
        report["aggregate_power_before"] = before;
        report["aggregate_power_after"] = power();
        report["execution_policy"] = json!("headless-worker");
        report["work_deadline_seconds"] = json!(10);
        report["cleanup_reserve_seconds"] = json!(10);
        report["output_directory"] = json!(request.output_directory);
        publish(
            &request.output_directory.join("receipt.json"),
            &serde_json::to_vec(&report).map_err(|error| error.to_string())?,
        )?;
        if recovered {
            fs::remove_file(request.output_directory.join("progress.json"))
                .map_err(|error| error.to_string())?;
        }
        for name in ["progress.json", "progress-next.json"] {
            let path = request.output_directory.join(name);
            if fs::metadata(&path).is_ok_and(|metadata| metadata.len() == 0) {
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
        Ok(report)
    }

    #[cfg(test)]
    mod tests {
        use super::{base, checkpoint, fs, json, recover_power};

        #[test]
        fn opencl_pq2_progress_recovers_completed_cpu_and_unfinished_gpu_power() -> crate::Result<()>
        {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "Bonsai-OpenClProgress-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir(&directory).map_err(|error| error.to_string())?;
            let mut progress = base();
            progress["cpu_power_before"] =
                json!({"ac_online":true,"battery_saver":false,"battery_percent":70});
            checkpoint(&directory, &progress)?;
            progress["cpu_power_after"] =
                json!({"ac_online":true,"battery_saver":false,"battery_percent":71});
            progress["gpu_power_before"] =
                json!({"ac_online":false,"battery_saver":true,"battery_percent":71});
            checkpoint(&directory, &progress)?;
            let mut report = base();
            report["aggregate_power_after"] =
                json!({"ac_online":true,"battery_saver":false,"battery_percent":72});
            assert!(recover_power(&directory, &mut report)?);
            assert_eq!(report["cpu_power_before"]["battery_percent"], 70);
            assert_eq!(report["cpu_power_after"]["battery_percent"], 71);
            assert_eq!(report["gpu_power_before"]["ac_online"], false);
            assert!(report["gpu_power_after"]["ac_online"].is_null());
            assert_eq!(report["performance_comparison_eligible"], false);
            assert!(directory.join("progress.json").is_file());
            progress["gpu_power_after"] =
                json!({"ac_online":"battery","battery_saver":false,"battery_percent":71});
            checkpoint(&directory, &progress)?;
            assert!(recover_power(&directory, &mut report).is_err());
            assert!(directory.join("progress.json").is_file());
            fs::remove_file(directory.join("progress.json"))
                .and_then(|()| fs::remove_dir(directory))
                .map_err(|error| error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{half, json, prepared, run, scalar, worker};

    fn q8k(codes: &[u8; 256], scale: f32) -> Vec<u8> {
        let mut bytes = scale.to_le_bytes().to_vec();
        bytes.extend_from_slice(codes);
        for values in codes.as_chunks::<16>().0 {
            let sum: i16 = values
                .iter()
                .map(|&value| i16::from(i8::from_ne_bytes([value])))
                .sum();
            bytes.extend_from_slice(&sum.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn opencl_pq2_control_preserves_four_symbols_signed_extremes_and_original_blocks()
    -> crate::Result<()> {
        let mut codes = [0_u8; 256];
        for group in codes.as_chunks_mut::<4>().0 {
            group.copy_from_slice(&[128, 127, 255, 1]);
        }
        let bytes = q8k(&codes, 0.5);
        let input = prepared(&bytes, 256)?;
        let mut weights = Vec::new();
        for (scale, symbols) in [
            (0x3c00_u16, 0xe4_u8),
            (0xbc00, 0x1b),
            (0x4000, 0xe4),
            (0x0001, 0x55),
        ] {
            weights.extend_from_slice(&scale.to_le_bytes());
            weights.extend_from_slice(&[symbols; 32]);
        }
        let (dots, output) = scalar(&weights, &input, 256, 2)?;
        // [symbol-1]=[-1,0,1,2] against [-128,127,-1,1]
        // gives 129 per packed byte; reversed symbols give -130.
        assert_eq!(dots, [4128, -4160, 4128, 0]);
        assert_eq!(output[0].to_bits(), 4144.0_f32.to_bits());
        assert_eq!(output[1].to_bits(), 4128.0_f32.to_bits());
        assert_eq!(
            half(&0x8000_u16.to_le_bytes())?.to_bits(),
            (-0.0_f32).to_bits()
        );
        assert_eq!(
            half(&0x0001_u16.to_le_bytes())?.to_bits(),
            (1.0_f32 / 16_777_216.0).to_bits()
        );
        Ok(())
    }

    #[test]
    fn opencl_pq2_control_rejects_partial_records_changed_sums_and_nonfinite_scales()
    -> crate::Result<()> {
        let bytes = q8k(&[0; 256], 1.0);
        for size in [0, 1, 291] {
            assert!(prepared(&bytes[..size], 256).is_err());
        }
        let mut invalid = bytes.clone();
        invalid[260] = 1;
        assert!(prepared(&invalid, 256).is_err());
        assert!(prepared(&q8k(&[0; 256], f32::INFINITY), 256).is_err());
        for bits in [0x7c00_u16, 0xfc00, 0x7e00] {
            assert!(half(&bits.to_le_bytes()).is_err());
        }
        let input = prepared(&bytes, 256)?;
        assert!(scalar(&[0; 34], &input, 256, 1).is_err());
        assert!(scalar(&[0; 68], &input, 128, 1).is_err());
        assert!(run(&json!({"execute":true})).is_err());
        assert!(worker(&json!({"driver":"custom.dll"})).is_err());
        Ok(())
    }
}
