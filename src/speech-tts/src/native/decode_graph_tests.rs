//! Former native lifetime obligations on the safe Rust cache.
#![forbid(unsafe_code)]

use std::{cell::RefCell, rc::Rc};

use crate::decode_graph::Cache;

#[derive(Default)]
struct State {
    allocations: usize,
    releases: usize,
    live: usize,
    before_build_live: Vec<usize>,
    released_during_compute: bool,
    computing: bool,
}

struct Graph {
    id: usize,
    width: i32,
    state: Rc<RefCell<State>>,
}

impl Drop for Graph {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        state.released_during_compute |= state.computing;
        state.releases += 1;
        state.live -= 1;
    }
}

#[derive(Clone)]
struct Builder {
    width: i32,
    state: Rc<RefCell<State>>,
    fail: bool,
    partial_failure: bool,
}

fn build(builder: &Builder) -> Option<Graph> {
    let mut state = builder.state.borrow_mut();
    let live = state.live;
    state.before_build_live.push(live);
    if builder.fail {
        return None;
    }
    state.allocations += 1;
    state.live += 1;
    let graph = Graph {
        id: state.allocations,
        width: builder.width,
        state: Rc::clone(&builder.state),
    };
    drop(state);
    if builder.partial_failure {
        // The actual native builder also cleans partial GGML allocations
        // before returning null; no incomplete graph transfers to Rust.
        drop(graph);
        return None;
    }
    Some(graph)
}

struct Owner(RefCell<Cache<Graph>>);

impl Owner {
    fn new() -> Result<Self, &'static str> {
        Ok(Self(RefCell::new(
            Cache::new(16).ok_or("Valid cache rejected")?,
        )))
    }

    fn begin(&self, tokens: i32, width: i32, builder: &mut Builder) -> Option<(usize, i32)> {
        builder.width = width;
        self.0
            .borrow_mut()
            .begin(tokens, width, || build(builder))
            .map(|graph| (graph.id, graph.width))
    }

    fn complete(&self) {
        assert!(self.0.borrow_mut().complete());
    }
}

fn id(graph: Option<(usize, i32)>) -> usize {
    assert!(graph.is_some());
    graph.map_or(0, |(id, _)| id)
}

fn graph_width(graph: Option<(usize, i32)>) -> i32 {
    assert!(graph.is_some());
    graph.map_or(0, |(_, width)| width)
}

fn builder() -> Builder {
    Builder {
        width: 0,
        state: Rc::default(),
        fail: false,
        partial_failure: false,
    }
}

#[test]
fn rust_owned_decode_cache_preserves_every_native_lifetime_assertion() -> Result<(), &'static str> {
    let cache = Owner::new()?;
    let mut builder = builder();
    let first = id(cache.begin(256, 1, &mut builder));
    cache.complete();
    assert_eq!(id(cache.begin(256, 1, &mut builder)), first);
    cache.complete();
    assert_eq!(builder.state.borrow().allocations, 1);
    assert_eq!(builder.state.borrow().releases, 0);
    for window in (512..=4096).step_by(256) {
        assert!(cache.begin(window, 1, &mut builder).is_some());
        cache.complete();
        let state = builder.state.borrow();
        assert_eq!(state.live, 1);
        assert_eq!(state.releases, state.allocations - 1);
    }
    assert_ne!(id(cache.begin(256, 1, &mut builder)), first);
    cache.complete();
    let before_width_change = builder.state.borrow().allocations;
    let wider = cache.begin(256, 2, &mut builder);
    assert_eq!(graph_width(wider), 2);
    cache.complete();
    assert_eq!(builder.state.borrow().allocations, before_width_change + 1);
    builder.fail = true;
    assert!(cache.begin(1024, 1, &mut builder).is_none());
    assert_eq!(builder.state.borrow().live, 0);
    assert_eq!(
        builder.state.borrow().releases,
        builder.state.borrow().allocations
    );
    builder.fail = false;
    assert!(cache.begin(1024, 1, &mut builder).is_some());
    cache.complete();
    builder.fail = true;
    assert!(cache.begin(1024, 2, &mut builder).is_none());
    assert_eq!(builder.state.borrow().live, 0);
    assert_eq!(
        builder.state.borrow().releases,
        builder.state.borrow().allocations
    );
    builder.fail = false;
    assert!(cache.begin(1024, 2, &mut builder).is_some());
    cache.complete();
    drop(cache);
    let state = builder.state.borrow();
    assert_eq!(state.live, 0);
    assert_eq!(state.releases, state.allocations);
    assert!(state.before_build_live.iter().all(|&live| live == 0));
    assert!(!state.released_during_compute);
    Ok(())
}

#[test]
fn rust_owned_decode_cache_preserves_compute_lease_partial_failure_and_isolation()
-> Result<(), &'static str> {
    let first = Owner::new()?;
    let second = Owner::new()?;
    let mut a = builder();
    let mut b = builder();
    assert!(first.begin(256, 1, &mut a).is_some());
    a.state.borrow_mut().computing = true;
    assert!(first.begin(4096, 2, &mut a).is_none());
    assert!(first.begin(256, 1, &mut a).is_none());
    assert_eq!(a.state.borrow().live, 1);
    assert_eq!(a.state.borrow().releases, 0);
    assert!(second.begin(4096, 2, &mut b).is_some());
    second.complete();
    a.state.borrow_mut().computing = false;
    first.complete();
    a.partial_failure = true;
    assert!(first.begin(512, 1, &mut a).is_none());
    assert_eq!(a.state.borrow().live, 0);
    assert_eq!(a.state.borrow().releases, a.state.borrow().allocations);
    assert_eq!(b.state.borrow().live, 1);
    a.partial_failure = false;
    assert!(first.begin(512, 1, &mut a).is_some());
    first.complete();
    drop(first);
    drop(second);
    assert_eq!(a.state.borrow().live, 0);
    assert_eq!(b.state.borrow().live, 0);
    assert!(!a.state.borrow().released_during_compute);
    Ok(())
}

#[test]
fn rust_owned_decode_cache_rejects_invalid_calls_without_build_or_eviction()
-> Result<(), &'static str> {
    assert!(Cache::<Graph>::new(0).is_none());
    let cache = Owner::new()?;
    let mut builder = builder();
    for (tokens, width) in [(0, 1), (-1, 1), (4097, 1), (256, 0), (256, -1)] {
        assert!(cache.begin(tokens, width, &mut builder).is_none());
    }
    assert_eq!(builder.state.borrow().before_build_live, [0_usize; 0]);
    assert!(cache.begin(256, 1, &mut builder).is_some());
    cache.complete();
    // A Rust builder closure and Graph's Drop are mandatory at the type boundary;
    // missing C callbacks and null owner handles no longer exist. Bad geometry
    // still must reject without evicting the completed cached resource.
    for (tokens, width) in [(0, 1), (-1, 1), (4097, 1), (256, 0), (256, -1)] {
        assert!(cache.begin(tokens, width, &mut builder).is_none());
    }
    assert_eq!(builder.state.borrow().live, 1);
    assert_eq!(builder.state.borrow().releases, 0);
    drop(cache);
    assert_eq!(builder.state.borrow().live, 0);
    Ok(())
}
