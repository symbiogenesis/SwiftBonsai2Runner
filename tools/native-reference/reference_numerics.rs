//! Independent, bounded arithmetic for native/public numerical references.
//!
//! This executable has no model runtime or third-party dependencies. Public
//! reconstruction uses an explicitly new scalar reduction recipe; historical
//! `NumPy` results retain their original identities.
#![forbid(unsafe_code)]

use std::env;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, message).into())
    }
}

fn half(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = (bits >> 10) & 31;
    let mantissa = u32::from(bits & 1023);
    if exponent == 0 {
        if mantissa == 0 {
            f32::from_bits(sign)
        } else {
            let v = f32::from(bits & 1023) * (1.0 / 16_777_216.0);
            if sign == 0 { v } else { -v }
        }
    } else if exponent == 31 {
        f32::from_bits(sign | 0x7f80_0000 | (mantissa << 13))
    } else {
        f32::from_bits(sign | (u32::from(exponent + 112) << 23) | (mantissa << 13))
    }
}

fn rounded_shift(value: u32, shift: u32) -> u32 {
    let base = value >> shift;
    let remainder = value & ((1_u32 << shift) - 1);
    let halfway = 1_u32 << (shift - 1);
    base + u32::from(remainder > halfway || (remainder == halfway && base & 1 == 1))
}

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "Masked IEEE fields and checked exponent branches bound these integer casts."
)]
fn to_half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 255) as i32 - 127;
    let mantissa = bits & 0x7f_ffff;
    if exponent > 15 {
        return sign | 0x7c00 | u16::from(mantissa != 0 && !value.is_finite());
    }
    if exponent < -25 {
        return sign;
    }
    if exponent < -14 {
        return sign | rounded_shift(mantissa | 0x80_0000, (-exponent - 1) as u32) as u16;
    }
    let rounded = rounded_shift(mantissa, 13);
    sign | ((((exponent + 15) as u16) << 10) + rounded as u16)
}

fn read_range(path: &str, offset: u64, length: usize) -> Result<Vec<u8>> {
    require(length <= 128 * 1024 * 1024, "numeric input exceeds bound")?;
    let mut file = File::open(path)?;
    let file_length = file.metadata()?.len();
    require(
        offset
            .checked_add(u64::try_from(length)?)
            .is_some_and(|end| end <= file_length),
        "numeric input extent differs",
    )?;
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

const fn float(bytes: &[u8]) -> f32 {
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn compare(args: &[String]) -> Result<String> {
    require(
        args.len() == 10,
        "compare requires expected actual format offset count stride absolute relative arithmetic",
    )?;
    let width = match args[3].as_str() {
        "f32" => 4,
        "f16" => 2,
        _ => return Err("unsupported scalar format".into()),
    };
    let offset: u64 = args[4].parse()?;
    let count: usize = args[5].parse()?;
    let stride: usize = args[6].parse()?;
    let absolute: f64 = args[7].parse()?;
    let relative: f64 = args[8].parse()?;
    require(
        count > 0
            && count <= 32 * 1024 * 1024
            && stride >= width
            && stride <= 8
            && absolute.is_finite()
            && relative.is_finite()
            && absolute >= 0.0
            && relative >= 0.0,
        "comparison controls differ",
    )?;
    require(
        args[9] == "fma" || args[9] == "plain",
        "comparison arithmetic differs",
    )?;
    let size = (count - 1)
        .checked_mul(stride)
        .and_then(|n| n.checked_add(width))
        .ok_or("comparison extent overflow")?;
    let expected = read_range(&args[1], offset, size)?;
    let actual = read_range(&args[2], 0, count * 4)?;
    require(
        fs::metadata(&args[2])?.len() == u64::try_from(count * 4)?,
        "actual component extent differs",
    )?;
    let mut failed = 0;
    let mut maximum = 0.0_f64;
    let mut exact = true;
    for index in 0..count {
        let position = index * stride;
        let reference = if width == 4 {
            float(&expected[position..])
        } else {
            half(u16::from_le_bytes([
                expected[position],
                expected[position + 1],
            ]))
        };
        let observed = float(&actual[index * 4..]);
        require(
            reference.is_finite() && observed.is_finite(),
            "nonfinite compared value",
        )?;
        exact &= reference.to_bits() == observed.to_bits();
        let difference = (f64::from(observed) - f64::from(reference)).abs();
        maximum = maximum.max(difference);
        #[allow(
            clippy::suboptimal_flops,
            reason = "The native receipt's original tolerance specifies separate multiply/add rounding."
        )]
        let limit = if args[9] == "fma" {
            relative.mul_add(f64::from(reference).abs(), absolute)
        } else {
            absolute + relative * f64::from(reference).abs()
        };
        failed += usize::from(difference > limit);
    }
    Ok(format!(
        "{{\"elements\":{count},\"failedElements\":{failed},\"maximumAbsoluteError\":{maximum},\"byteExact\":{exact}}}"
    ))
}

fn normalize(args: &[String]) -> Result<String> {
    require(args.len() == 3, "normalize requires input output")?;
    require(
        fs::metadata(&args[1])?.len() == 5120 * 4,
        "direction geometry differs",
    )?;
    let bytes = read_range(&args[1], 0, 5120 * 4)?;
    let values: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    require(values.iter().all(|v| v.is_finite()), "nonfinite direction")?;
    let sum: f64 = values.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
    // The new recipe uses f64 scalar norm then a single f32 rounding. It never
    // labels this normalization or the scalar dot as the previous BLAS recipe.
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "The declared reference recipe rounds the scalar f64 norm once to binary32."
    )]
    let norm = sum.sqrt() as f32;
    require(
        norm.is_finite() && (norm - 1.0).abs() <= 1e-5,
        "selected direction is not unit normalized",
    )?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    for value in values {
        output.write_all(&(value / norm).to_le_bytes())?;
    }
    output.flush()?;
    Ok(format!("{{\"norm\":{norm}}}"))
}

fn walsh_hadamard(row: &mut [f32]) {
    for block in row.as_chunks_mut::<1024>().0 {
        for span in [1, 2, 4, 8, 16, 32, 64, 128, 256, 512] {
            for group in block.chunks_exact_mut(span * 2) {
                let (left, right) = group.split_at_mut(span);
                for (a, b) in left.iter_mut().zip(right) {
                    let first = *a;
                    *a = first + *b;
                    *b = first - *b;
                }
            }
        }
    }
}

fn reconstruct(args: &[String]) -> Result<String> {
    require(args.len() == 12, "reconstruct controls differ")?;
    let rows: usize = args[6].parse()?;
    let width: usize = args[7].parse()?;
    require(
        rows > 0 && rows <= 5120 && width > 0 && width <= 6144 && width % 1024 == 0,
        "affine tensor geometry differs",
    )?;
    let weight = read_range(&args[1], args[2].parse()?, rows * width / 4)?;
    let scales = read_range(&args[1], args[3].parse()?, rows * width / 64)?;
    let biases = read_range(&args[1], args[4].parse()?, rows * width / 64)?;
    let sign_width = match args[11].as_str() {
        "F16" => 2,
        "F32" => 4,
        _ => return Err("sign scalar format differs".into()),
    };
    let signs_raw = read_range(&args[1], args[5].parse()?, width * sign_width)?;
    let signs: Vec<f32> = signs_raw
        .chunks_exact(sign_width)
        .map(|b| {
            if sign_width == 4 {
                float(b)
            } else {
                half(u16::from_le_bytes([b[0], b[1]]))
            }
        })
        .collect();
    require(
        signs
            .iter()
            .all(|v| matches!(v.to_bits(), 0x3f80_0000 | 0xbf80_0000)),
        "invalid per-site sign vector",
    )?;
    let direction_raw = read_range(&args[8], 0, rows * 4)?;
    require(
        fs::metadata(&args[8])?.len() == u64::try_from(rows * 4)?,
        "direction dimension differs",
    )?;
    let direction: Vec<f32> = direction_raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    require(
        direction.iter().all(|v| v.is_finite()),
        "nonfinite direction",
    )?;
    let mut stored = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[10])?,
    );
    let mut result = vec![0.0_f32; width];
    let mut row = vec![0.0_f32; width];
    for (row_index, factor) in direction.iter().enumerate() {
        for (column, value) in row.iter_mut().enumerate() {
            let element = row_index * width + column;
            let q = (weight[element / 4] >> ((element % 4) * 2)) & 3;
            let group = element / 128;
            let scale_bits = u16::from_le_bytes([scales[group * 2], scales[group * 2 + 1]]);
            let bias_bits = u16::from_le_bytes([biases[group * 2], biases[group * 2 + 1]]);
            let scale = half(scale_bits);
            require(
                q <= 2
                    && scale.is_finite()
                    && scale.abs() <= 32752.0
                    && bias_bits == (scale_bits ^ 0x8000),
                "affine code/bias/scale differs",
            )?;
            // Ternary affine products and addition are exact in this domain.
            *value = half(to_half(
                half(to_half(f32::from(q) * scale)) + half(bias_bits),
            ));
        }
        walsh_hadamard(&mut row);
        for column in 0..width {
            let transformed = (row[column] * (1.0 / 32.0)) * signs[column];
            let bits = to_half(transformed);
            let value = half(bits);
            require(value.is_finite(), "candidate F16 storage overflow")?;
            stored.write_all(&bits.to_le_bytes())?;
            let product = *factor * value;
            result[column] += product;
        }
    }
    stored.flush()?;
    require(
        result.iter().all(|v| v.is_finite()),
        "nonfinite reconstructed A",
    )?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[9])?;
    for value in result {
        output.write_all(&value.to_le_bytes())?;
    }
    output.flush()?;
    Ok(format!("{{\"rows\":{rows},\"width\":{width}}}"))
}

fn run() -> Result<String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("compare") => compare(&args),
        Some("normalize") => normalize(&args),
        Some("reconstruct") => reconstruct(&args),
        Some("row") if args.len() == 4 => {
            let count: usize = args[3].parse()?;
            require(count > 0 && count <= 248_320, "row count differs")?;
            let left = read_range(&args[1], 0, count * 4)?;
            let right = read_range(&args[2], 0, count * 4)?;
            require(
                fs::metadata(&args[1])?.len() == u64::try_from(count * 4)?
                    && fs::metadata(&args[2])?.len() == u64::try_from(count * 4)?,
                "row extent differs",
            )?;
            let mut maxima = [f32::NEG_INFINITY; 2];
            let mut indices = [0; 2];
            for (side, bytes) in [&left, &right].into_iter().enumerate() {
                for (index, chunk) in bytes.as_chunks::<4>().0.iter().enumerate() {
                    let value = float(chunk);
                    require(value.is_finite(), "nonfinite row")?;
                    if value > maxima[side] {
                        maxima[side] = value;
                        indices[side] = index;
                    }
                }
            }
            Ok(format!(
                "{{\"elements\":{count},\"byteExact\":{},\"expectedGreedyToken\":{},\"actualGreedyToken\":{}}}",
                left == right,
                indices[0],
                indices[1]
            ))
        }
        Some("self-check") if args.len() == 1 => {
            let mut affine_cases = 0_u32;
            for bits in 0..=u16::MAX {
                let value = half(bits);
                if value.is_finite() {
                    require(to_half(value) == bits, "binary16 roundtrip differs")?;
                    if value.abs() <= 32752.0 {
                        for code in 0_u8..=2 {
                            let product = half(to_half(f32::from(code) * value));
                            let actual = to_half(product + half(bits ^ 0x8000));
                            let expected = to_half(f32::from(code).mul_add(value, -value));
                            require(actual == expected, "binary16 affine/FMA oracle differs")?;
                            affine_cases += 1;
                        }
                    }
                }
            }
            require(
                to_half(1.000_488_3) == 0x3c00 && to_half(1.001_464_8) == 0x3c02,
                "binary16 ties-to-even differs",
            )?;
            Ok(format!(
                "{{\"binary16Roundtrips\":63488,\"affineCases\":{affine_cases},\"passed\":true}}"
            ))
        }
        _ => Err("unknown bounded reference operation".into()),
    }
}

fn main() -> std::process::ExitCode {
    run().map_or_else(
        |_| {
            let _ = writeln!(
                io::stderr().lock(),
                "Reference arithmetic rejected its bounded inputs."
            );
            std::process::ExitCode::FAILURE
        },
        |report| {
            if writeln!(io::stdout().lock(), "{report}").is_ok() {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        },
    )
}
