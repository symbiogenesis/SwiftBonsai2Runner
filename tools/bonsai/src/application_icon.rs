//! Regenerates the native application icon from its bounded two-leaf SVG source.
#![forbid(unsafe_code)]

use flate2::{Compression, write::ZlibEncoder};
use std::{fs, io::Write as _, path::Path, process::ExitCode};

type Result<T> = std::result::Result<T, String>;
const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
const SCALE: u32 = 4;

#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}
struct Shape {
    points: Vec<Point>,
    fill: bool,
    stroke: f64,
    closed: bool,
}

fn attribute<'a>(tag: &'a str, name: &str) -> Result<Option<&'a str>> {
    let prefix = format!("{name}=\"");
    let mut values = tag.match_indices(&prefix);
    let Some((index, _)) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(format!("Duplicate SVG attribute: {name}"));
    }
    Ok(Some(
        tag[index + prefix.len()..]
            .split_once('"')
            .ok_or("Unterminated SVG attribute.")?
            .0,
    ))
}

fn number(tokens: &[&str], index: &mut usize) -> Result<f64> {
    let value = tokens
        .get(*index)
        .ok_or("Incomplete SVG coordinate.")?
        .parse::<f64>()
        .map_err(|_| "Invalid SVG coordinate.")?;
    *index += 1;
    if !value.is_finite() || !(0.0..=24.0).contains(&value) {
        return Err("SVG coordinate outside native 24-unit mark.".into());
    }
    Ok(value)
}
fn point(tokens: &[&str], index: &mut usize) -> Result<Point> {
    Ok(Point {
        x: number(tokens, index)?,
        y: number(tokens, index)?,
    })
}
fn cubic(a: Point, b: Point, c: Point, d: Point, t: f64) -> Point {
    let u = 1.0 - t;
    Point {
        x: u * u * u * a.x + 3.0 * u * u * t * b.x + 3.0 * u * t * t * c.x + t * t * t * d.x,
        y: u * u * u * a.y + 3.0 * u * u * t * b.y + 3.0 * u * t * t * c.y + t * t * t * d.y,
    }
}

// Accept the repository's small SVG dialect explicitly. Unsupported geometry,
// transforms and external resources must never silently change the native mark.
fn parse(source: &str) -> Result<Vec<Shape>> {
    if source.len() > 16_384
        || source.contains(['&', '!'])
        || !source.trim_start().starts_with("<svg ")
        || !source.trim_end().ends_with("</svg>")
    {
        return Err("Unsupported application SVG root or extent.".into());
    }
    let mut shapes = Vec::new();
    for element in source.split('<').skip(1) {
        let (tag, trailing) = element.split_once('>').ok_or("Unterminated SVG tag.")?;
        if !trailing.trim().is_empty() {
            return Err("SVG text nodes unsupported.".into());
        }
        if tag == "/svg" {
            continue;
        }
        if tag.starts_with("svg ") {
            if attribute(tag, "viewBox")? != Some("0 0 24 24") {
                return Err("SVG requires viewBox 0 0 24 24.".into());
            }
            continue;
        }
        if !tag.starts_with("path ") || !tag.ends_with('/') || shapes.len() >= 8 {
            return Err("Only bounded self-closing SVG paths supported.".into());
        }
        let mut rest = tag.trim_start_matches("path ").trim_end_matches('/').trim();
        while !rest.is_empty() {
            let (name, value) = rest.split_once("=\"").ok_or("Invalid SVG attribute.")?;
            if !["d", "fill", "stroke", "stroke-width"].contains(&name) {
                return Err(format!("Unsupported SVG attribute: {name}"));
            }
            rest = value
                .split_once('"')
                .ok_or("Unterminated SVG attribute.")?
                .1
                .trim_start();
        }
        let fill = match attribute(tag, "fill")? {
            Some("#2e8555") => true,
            Some("none") => false,
            _ => return Err("SVG fill must use native branding color.".into()),
        };
        let stroke = match (attribute(tag, "stroke")?, attribute(tag, "stroke-width")?) {
            (None, None) => 0.0,
            (Some("#2e8555"), Some(width)) => {
                let width = width
                    .parse::<f64>()
                    .map_err(|_| "Invalid SVG stroke width.")?;
                if !width.is_finite() || !(0.0..=4.0).contains(&width) {
                    return Err("SVG stroke exceeds bound.".into());
                }
                width
            }
            _ => return Err("Unsupported SVG stroke.".into()),
        };
        let tokens: Vec<_> = attribute(tag, "d")?
            .ok_or("Missing SVG path.")?
            .split_whitespace()
            .collect();
        if tokens.len() > 128 || tokens.first() != Some(&"M") {
            return Err("SVG paths require one bounded move.".into());
        }
        let mut index = 1;
        let mut points = vec![point(&tokens, &mut index)?];
        let mut closed = false;
        while index < tokens.len() {
            let command = tokens[index];
            index += 1;
            match command {
                "C" if !closed => {
                    let a = *points.last().ok_or("Missing SVG move.")?;
                    let b = point(&tokens, &mut index)?;
                    let c = point(&tokens, &mut index)?;
                    let d = point(&tokens, &mut index)?;
                    for step in 1..=128 {
                        points.push(cubic(a, b, c, d, f64::from(step) / 128.0));
                    }
                }
                "Z" if !closed && index == tokens.len() => {
                    closed = true;
                }
                _ => return Err(format!("Unsupported SVG command: {command}")),
            }
        }
        if points.len() < 2 || (fill && !closed) || (!fill && stroke == 0.0) {
            return Err("SVG path has no visible geometry.".into());
        }
        shapes.push(Shape {
            points,
            fill,
            stroke,
            closed,
        });
    }
    if shapes.is_empty() {
        return Err("SVG paths missing.".into());
    }
    Ok(shapes)
}

fn segments(shape: &Shape) -> impl Iterator<Item = (Point, Point)> + '_ {
    shape
        .points
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .chain(
            shape
                .closed
                .then(|| (shape.points[shape.points.len() - 1], shape.points[0])),
        )
}
fn inside(shape: &Shape, p: Point) -> bool {
    let mut winding = 0_i32;
    for (a, b) in segments(shape) {
        let cross = (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y);
        if a.y <= p.y && b.y > p.y && cross > 0.0 {
            winding += 1;
        }
        if a.y > p.y && b.y <= p.y && cross < 0.0 {
            winding -= 1;
        }
    }
    winding != 0
}
fn on_stroke(shape: &Shape, p: Point) -> bool {
    segments(shape).any(|(a, b)| {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let length = dx * dx + dy * dy;
        if length == 0.0 {
            return false;
        }
        let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / length;
        if !(0.0..=1.0).contains(&t) {
            return false;
        }
        let x = p.x - (a.x + t * dx);
        let y = p.y - (a.y + t * dy);
        x * x + y * y <= shape.stroke * shape.stroke / 4.0
    })
}
fn rasterize(shapes: &[Shape], size: u32) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    let samples = SCALE * SCALE;
    for y in 0..size {
        raw.push(0); // PNG scanline filter: None.
        for x in 0..size {
            let mut coverage = 0_u32;
            for sy in 0..SCALE {
                for sx in 0..SCALE {
                    let p = Point {
                        x: (f64::from(x * SCALE + sx) + 0.5) * 24.0 / f64::from(size * SCALE),
                        y: (f64::from(y * SCALE + sy) + 0.5) * 24.0 / f64::from(size * SCALE),
                    };
                    if shapes
                        .iter()
                        .any(|s| (s.fill && inside(s, p)) || (s.stroke > 0.0 && on_stroke(s, p)))
                    {
                        coverage += 1;
                    }
                }
            }
            raw.extend_from_slice(&[
                0x2e,
                0x85,
                0x55,
                u8::try_from((coverage * 255 + samples / 2) / samples)
                    .map_err(|e| e.to_string())?,
            ]);
        }
    }
    Ok(raw)
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], bytes: &[u8]) -> Result<()> {
    png.extend_from_slice(
        &u32::try_from(bytes.len())
            .map_err(|e| e.to_string())?
            .to_be_bytes(),
    );
    let start = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(bytes);
    png.extend_from_slice(&crc32(&png[start..]).to_be_bytes());
    Ok(())
}
fn png(shapes: &[Shape], size: u32) -> Result<Vec<u8>> {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::from(size.to_be_bytes());
    header.extend_from_slice(&size.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &header)?;
    let mut compressed = ZlibEncoder::new(Vec::new(), Compression::best());
    compressed
        .write_all(&rasterize(shapes, size)?)
        .map_err(|e| e.to_string())?;
    chunk(
        &mut png,
        b"IDAT",
        &compressed.finish().map_err(|e| e.to_string())?,
    )?;
    chunk(&mut png, b"IEND", &[])?;
    Ok(png)
}
fn ico(frames: &[Vec<u8>]) -> Result<Vec<u8>> {
    if frames.len() != SIZES.len() {
        return Err("ICO frame inventory differs.".into());
    }
    let mut ico = vec![0, 0, 1, 0, 7, 0];
    let mut offset = 118_u32;
    for (size, frame) in SIZES.into_iter().zip(frames) {
        let dimension = u8::try_from(size % 256).map_err(|e| e.to_string())?;
        ico.extend_from_slice(&[dimension, dimension, 0, 0, 1, 0, 32, 0]);
        let length = u32::try_from(frame.len()).map_err(|e| e.to_string())?;
        ico.extend_from_slice(&length.to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset = offset.checked_add(length).ok_or("ICO length overflow.")?;
    }
    for frame in frames {
        ico.extend_from_slice(frame);
    }
    Ok(ico)
}
fn generate(directory: &Path) -> Result<()> {
    let file = fs::File::open(directory.join("bonsai.svg")).map_err(|e| e.to_string())?;
    let mut source = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(file, 16_385), &mut source)
        .map_err(|e| e.to_string())?;
    let shapes = parse(&source)?;
    let frames = SIZES
        .into_iter()
        .map(|size| png(&shapes, size))
        .collect::<Result<Vec<_>>>()?;
    fs::write(directory.join("bonsai.ico"), ico(&frames)?).map_err(|e| e.to_string())?;
    fs::write(directory.join("bonsai.png"), &frames[frames.len() - 1])
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command, option, directory] if command == "icons" && option == "--assets" => {
            generate(Path::new(directory))
        }
        _ => Err("Usage: bonsai-assets icons --assets DIRECTORY".into()),
    }
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;
    const SOURCE: &str = include_str!("../../../src/chat-ui/assets/bonsai.svg");
    #[test]
    fn icon_rejects_unsupported_geometry_and_changed_branding() {
        assert_eq!(parse(SOURCE).ok().map(|s| s.len()), Some(3));
        for changed in [
            SOURCE.replace("C ", "L "),
            SOURCE.replace("#2e8555", "#ffffff"),
            SOURCE.replace("fill=", "transform="),
            SOURCE.replace("21.5", "999"),
            SOURCE.replace("24 24", "48 48"),
        ] {
            assert!(parse(&changed).is_err());
        }
    }
    #[test]
    fn icon_png_has_valid_chunks_transparency_and_native_color() -> Result<()> {
        let frame = png(&parse(SOURCE)?, 16)?;
        assert_eq!(&frame[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&frame[16..24], &[0, 0, 0, 16, 0, 0, 0, 16]);
        let mut cursor = 8;
        let mut payload = Vec::new();
        while cursor < frame.len() {
            let length = usize::try_from(u32::from_be_bytes(
                frame[cursor..cursor + 4].try_into().map_err(|_| "chunk")?,
            ))
            .map_err(|e| e.to_string())?;
            let end = cursor + 8 + length;
            assert_eq!(
                crc32(&frame[cursor + 4..end]),
                u32::from_be_bytes(frame[end..end + 4].try_into().map_err(|_| "CRC")?)
            );
            if &frame[cursor + 4..cursor + 8] == b"IDAT" {
                flate2::read::ZlibDecoder::new(&frame[cursor + 8..end])
                    .read_to_end(&mut payload)
                    .map_err(|e| e.to_string())?;
            }
            cursor = end + 4;
        }
        assert_eq!(payload.len(), 16 * 65);
        let pixels: Vec<_> = payload
            .chunks_exact(65)
            .flat_map(|row| row[1..].chunks_exact(4))
            .collect();
        assert!(pixels.iter().all(|p| p[..3] == [0x2e, 0x85, 0x55]));
        for alpha in [0, 255] {
            assert!(pixels.iter().any(|p| p[3] == alpha));
        }
        assert!(pixels.iter().any(|p| p[3] > 0 && p[3] < 255));
        Ok(())
    }
    #[test]
    fn icon_ico_preserves_all_frame_sizes_offsets_and_payloads() -> Result<()> {
        let frames: Vec<_> = SIZES
            .into_iter()
            .map(|s| s.to_le_bytes().to_vec())
            .collect();
        let icon = ico(&frames)?;
        assert_eq!(&icon[..6], &[0, 0, 1, 0, 7, 0]);
        for (index, size) in SIZES.into_iter().enumerate() {
            let start = 6 + index * 16;
            assert_eq!(u32::from(icon[start]), size % 256);
            assert_eq!(icon[start], icon[start + 1]);
            assert_eq!(&icon[start + 4..start + 8], &[1, 0, 32, 0]);
            let offset = usize::try_from(u32::from_le_bytes(
                icon[start + 12..start + 16]
                    .try_into()
                    .map_err(|_| "offset")?,
            ))
            .map_err(|e| e.to_string())?;
            assert_eq!(&icon[offset..offset + 4], &frames[index]);
        }
        Ok(())
    }
}
