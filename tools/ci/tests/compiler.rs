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

struct SlotCache {
    slots: [(u64, Vec<u8>); 2],
}

impl SlotCache {
    // Loop-carried mutable lending: a hit returns an immutable borrow from the
    // mutable slice iteration; a miss reuses `&mut self` after the loop ends.
    fn get_or_insert(&mut self, key: u64, fill: &[u8]) -> &[u8] {
        for (slot_key, value) in &mut self.slots {
            if *slot_key == key {
                return value.as_slice();
            }
        }
        self.slots[0] = (key, fill.to_vec());
        self.slots[0].1.as_slice()
    }
}

#[test]
fn polonius_lends_from_mutable_slice_loop_with_fallback_mutation() {
    let mut cache = SlotCache {
        slots: [(1, b"first".to_vec()), (2, b"second".to_vec())],
    };
    assert_eq!(cache.get_or_insert(2, b"second_unused"), b"second");
    assert_eq!(cache.get_or_insert(3, b"third"), b"third");
    assert_eq!(cache.get_or_insert(3, b"third_unused"), b"third");
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

trait LendingBackend {
    type View<'a>: AsRef<[u8]>
    where
        Self: 'a;

    fn lend(&mut self) -> Option<Self::View<'_>>;
}

struct SliceBackend {
    chunks: [&'static [u8]; 2],
    cursor: usize,
}

impl LendingBackend for SliceBackend {
    type View<'a> = &'a [u8];

    fn lend(&mut self) -> Option<Self::View<'_>> {
        let chunk = *self.chunks.get(self.cursor)?;
        self.cursor += 1;
        Some(chunk)
    }
}

trait SupportsView {
    fn visit(&mut self) -> usize;
}

// Higher-ranked associated-type projection normalization: `B::View<'a>` inside
// `for<'a> fn(B::View<'a>) -> usize` normalizes without helper traits.
impl<B: LendingBackend> SupportsView for (B, for<'a> fn(B::View<'a>) -> usize) {
    fn visit(&mut self) -> usize {
        let mut total = 0;
        while let Some(view) = self.0.lend() {
            total += (self.1)(view);
        }
        total
    }
}

fn consume_lending<B: LendingBackend>(
    backend: &mut B,
    mut visit: impl for<'a> FnMut(B::View<'a>) -> usize,
) -> usize {
    let mut total = 0;
    while let Some(view) = backend.lend() {
        total += visit(view);
    }
    total
}

const fn view_len(bytes: &[u8]) -> usize {
    bytes.len()
}

#[test]
fn next_solver_normalizes_higher_ranked_lending_views() {
    let measure: for<'a> fn(&'a [u8]) -> usize = view_len;
    let mut pair = (
        SliceBackend {
            chunks: [b"next", b"solver"],
            cursor: 0,
        },
        measure,
    );
    assert_eq!(pair.visit(), 10);

    let mut backend = SliceBackend {
        chunks: [b"gat", b"view"],
        cursor: 0,
    };
    assert_eq!(consume_lending(&mut backend, <[u8]>::len), 7);
}
