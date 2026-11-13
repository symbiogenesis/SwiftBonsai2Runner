//! Independent PE extent and API-set selection controls.

use super::{Aliases, Result, imports, normalize_aliases, parse_image, resolve};

fn put16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn image() -> Vec<u8> {
    let mut data = vec![0; 0x1400];
    data[..2].copy_from_slice(b"MZ");
    put32(&mut data, 60, 0x80);
    put32(&mut data, 0x80, 0x4550);
    put16(&mut data, 0x84, 0xaa64);
    put16(&mut data, 0x86, 1);
    put16(&mut data, 0x94, 240);
    put16(&mut data, 0x98, 0x20b);
    put32(&mut data, 0x98 + 108, 16);
    data[0x188..0x18e].copy_from_slice(b".rdata");
    for offset in [0x190, 0x194, 0x198] {
        put32(&mut data, offset, 0x1000);
    }
    put32(&mut data, 0x19c, 0x400);
    data
}

#[test]
fn static_arm64_import_bounds_reject_virtual_tail_and_unterminated_tables() -> Result<()> {
    let mut data = image();
    put32(&mut data, 0x98 + 120, 0x1000);
    put32(&mut data, 0x98 + 124, 40);
    put32(&mut data, 0x40c, 0x1100);
    data[0x500..0x50d].copy_from_slice(b"kernel32.dll\0");
    assert_eq!(
        imports(&parse_image(data.clone())?, false)?,
        ["kernel32.dll"]
    );
    put32(&mut data, 0x98 + 124, 20);
    assert_eq!(
        imports(&parse_image(data.clone())?, false),
        Err("Unterminated import directory".into())
    );
    data.resize(0x2400, 0);
    put32(&mut data, 0x98 + 120, 0x1ff0);
    put32(&mut data, 0x98 + 124, 40);
    assert_eq!(
        imports(&parse_image(data)?, false),
        Err("PE RVA extent outside raw section".into())
    );
    Ok(())
}

#[test]
fn static_arm64_headers_and_delay_va_are_checked() -> Result<()> {
    let mut data = image();
    put16(&mut data, 0x84, 0x8664);
    assert!(parse_image(data).is_err());
    let mut data = image();
    put16(&mut data, 0x94, 111);
    assert!(parse_image(data).is_err());
    let mut data = image();
    put32(&mut data, 0x98 + 108, 17);
    assert!(parse_image(data).is_err());
    let mut data = image();
    put32(&mut data, 0x98 + 112 + 13 * 8, 0x1000);
    put32(&mut data, 0x98 + 116 + 13 * 8, 64);
    put32(&mut data, 0x98 + 24, 0x1000);
    put32(&mut data, 0x404, 0x2100);
    data[0x500..0x50d].copy_from_slice(b"kernel32.dll\0");
    assert_eq!(
        imports(&parse_image(data.clone())?, true)?,
        ["kernel32.dll"]
    );
    put32(&mut data, 0x404, 1);
    assert!(imports(&parse_image(data)?, true).is_err());
    Ok(())
}

#[test]
fn static_api_set_alias_selection_preserves_importer_and_ambiguity_rules() -> Result<()> {
    let dependency = "api-ms-win-core-test-l1-1-0.dll";
    let aliases = Aliases::from([(
        "api-ms-win-core-test-l1-1-*.dll".into(),
        vec![
            (String::new(), "default.dll".into()),
            ("caller".into(), "selected.dll".into()),
        ],
    )]);
    assert_eq!(resolve(&aliases, dependency, "caller.dll")?, "selected.dll");
    assert_eq!(resolve(&aliases, dependency, "other.dll")?, "default.dll");
    let aliases = Aliases::from([(dependency.into(), Vec::new())]);
    assert!(resolve(&aliases, dependency, "caller.dll").is_err());
    let aliases = normalize_aliases(Aliases::from([(
        dependency.to_uppercase(),
        vec![(String::new(), "selected.dll".into())],
    )]))?;
    assert_eq!(resolve(&aliases, dependency, "caller.dll")?, "selected.dll");
    assert!(
        normalize_aliases(Aliases::from([
            (dependency.into(), Vec::new()),
            (
                dependency.to_uppercase(),
                vec![(String::new(), "selected.dll".into())]
            ),
        ]))
        .is_err()
    );
    let aliases = Aliases::from([(
        dependency.into(),
        vec![
            (String::new(), "one.dll".into()),
            (String::new(), "two.dll".into()),
        ],
    )]);
    assert!(resolve(&aliases, dependency, "caller.dll").is_err());
    Ok(())
}
