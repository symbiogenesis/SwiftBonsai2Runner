//! Behavioral gates for the compiler analyses required by the pinned nightly.
//!
//! Foundation compiles these tests through Cargo on hosted Linux and Windows.
//! Deliberately use the inherited compiler configuration, without opt-in flags:
//! disabling either analysis must fail compilation before any test can run.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt::Debug;

// Flow-sensitive lending: the first borrow escapes only on the occupied path.
// https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/
fn existing_or_default(values: &mut HashMap<u8, String>, key: u8) -> &mut String {
    if let Some(value) = values.get_mut(&key) {
        return value;
    }
    values.entry(key).or_default()
}

#[test]
fn polonius_lends_existing_or_new_storage() {
    let mut values = HashMap::from([(1, String::from("existing"))]);
    existing_or_default(&mut values, 1).push('!');
    existing_or_default(&mut values, 2).push_str("new");

    assert_eq!(values.len(), 2);
    assert_eq!(values.get(&1).map(String::as_str), Some("existing!"));
    assert_eq!(values.get(&2).map(String::as_str), Some("new"));
}

// The recursive opaque return is normalized inside its defining function.
// Debug/PartialEq expose the result to assertions, without declaring Add.
// https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/
fn recursive_opaque(recursive: bool) -> impl Sized + Debug + PartialEq<u8> {
    if recursive {
        recursive_opaque(false) + 1_u8
    } else {
        0_u8
    }
}

#[test]
fn next_solver_normalizes_recursive_opaque_return() {
    assert_eq!(recursive_opaque(false), 0_u8);
    assert_eq!(recursive_opaque(true), 1_u8);
}
