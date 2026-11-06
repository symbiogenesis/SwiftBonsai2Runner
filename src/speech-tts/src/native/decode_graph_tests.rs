//! Exact former lifetime obligations on the production Rust cache ABI.
#![forbid(unsafe_code)]

use std::{cell::RefCell, ffi::c_void, rc::Rc};

use super::decode_graph::{
    bonsai_qt_decode_cache_begin, bonsai_qt_decode_cache_complete, bonsai_qt_decode_cache_free,
    bonsai_qt_decode_cache_new,
};

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

std::thread_local! {
    static ACTIVE_BUILDER: RefCell<Option<Builder>> = const { RefCell::new(None) };
    static GRAPHS: RefCell<Vec<Option<Graph>>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn build(data: *mut c_void) -> *mut c_void {
    if data.addr() != 1 {
        return std::ptr::null_mut();
    }
    let Some(builder) = ACTIVE_BUILDER.with(|active| active.borrow().clone()) else {
        return std::ptr::null_mut();
    };
    let mut state = builder.state.borrow_mut();
    let live = state.live;
    state.before_build_live.push(live);
    if builder.fail {
        return std::ptr::null_mut();
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
        return std::ptr::null_mut();
    }
    let handle = GRAPHS.with(|graphs| {
        let mut graphs = graphs.borrow_mut();
        graphs.push(Some(graph));
        graphs.len()
    });
    std::ptr::without_provenance_mut(handle)
}

extern "C" fn destroy(graph: *mut c_void) {
    let slot = graph.addr().checked_sub(1);
    GRAPHS.with(|graphs| {
        let mut graphs = graphs.borrow_mut();
        if let Some(entry) = slot.and_then(|index| graphs.get_mut(index)) {
            let _ = entry.take();
        }
    });
}

struct Owner(*mut c_void);

impl Owner {
    fn new() -> Self {
        Self(bonsai_qt_decode_cache_new(16))
    }

    fn begin(&self, tokens: i32, width: i32, builder: &mut Builder) -> *mut c_void {
        builder.width = width;
        ACTIVE_BUILDER.with(|active| *active.borrow_mut() = Some(builder.clone()));
        let pointer = bonsai_qt_decode_cache_begin(
            self.0,
            tokens,
            width,
            std::ptr::without_provenance_mut(1),
            Some(build),
            Some(destroy),
        );
        ACTIVE_BUILDER.with(|active| *active.borrow_mut() = None);
        pointer
    }

    fn complete(&self) {
        bonsai_qt_decode_cache_complete(self.0);
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        bonsai_qt_decode_cache_free(self.0);
    }
}

fn id(graph: *mut c_void) -> usize {
    assert!(!graph.is_null());
    let index = graph.addr() - 1;
    GRAPHS.with(|graphs| {
        graphs
            .borrow()
            .get(index)
            .and_then(Option::as_ref)
            .map_or(0, |entry| entry.id)
    })
}

fn graph_width(graph: *mut c_void) -> i32 {
    assert!(!graph.is_null());
    let index = graph.addr() - 1;
    GRAPHS.with(|graphs| {
        graphs
            .borrow()
            .get(index)
            .and_then(Option::as_ref)
            .map_or(0, |entry| entry.width)
    })
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
fn rust_owned_decode_cache_preserves_every_native_lifetime_assertion() {
    let cache = Owner::new();
    assert!(!cache.0.is_null());
    let mut builder = builder();
    let first = id(cache.begin(256, 1, &mut builder));
    cache.complete();
    assert_eq!(id(cache.begin(256, 1, &mut builder)), first);
    cache.complete();
    assert_eq!(builder.state.borrow().allocations, 1);
    assert_eq!(builder.state.borrow().releases, 0);
    for window in (512..=4096).step_by(256) {
        assert!(!cache.begin(window, 1, &mut builder).is_null());
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
    assert!(cache.begin(1024, 1, &mut builder).is_null());
    assert_eq!(builder.state.borrow().live, 0);
    assert_eq!(
        builder.state.borrow().releases,
        builder.state.borrow().allocations
    );
    builder.fail = false;
    assert!(!cache.begin(1024, 1, &mut builder).is_null());
    cache.complete();
    builder.fail = true;
    assert!(cache.begin(1024, 2, &mut builder).is_null());
    assert_eq!(builder.state.borrow().live, 0);
    assert_eq!(
        builder.state.borrow().releases,
        builder.state.borrow().allocations
    );
    builder.fail = false;
    assert!(!cache.begin(1024, 2, &mut builder).is_null());
    cache.complete();
    drop(cache);
    let state = builder.state.borrow();
    assert_eq!(state.live, 0);
    assert_eq!(state.releases, state.allocations);
    assert!(state.before_build_live.iter().all(|&live| live == 0));
    assert!(!state.released_during_compute);
}

#[test]
fn rust_owned_decode_cache_preserves_compute_lease_partial_failure_and_isolation() {
    let first = Owner::new();
    let second = Owner::new();
    let mut a = builder();
    let mut b = builder();
    assert!(!first.begin(256, 1, &mut a).is_null());
    a.state.borrow_mut().computing = true;
    assert!(first.begin(4096, 2, &mut a).is_null());
    assert!(first.begin(256, 1, &mut a).is_null());
    assert_eq!(a.state.borrow().live, 1);
    assert_eq!(a.state.borrow().releases, 0);
    assert!(!second.begin(4096, 2, &mut b).is_null());
    second.complete();
    a.state.borrow_mut().computing = false;
    first.complete();
    a.partial_failure = true;
    assert!(first.begin(512, 1, &mut a).is_null());
    assert_eq!(a.state.borrow().live, 0);
    assert_eq!(a.state.borrow().releases, a.state.borrow().allocations);
    assert_eq!(b.state.borrow().live, 1);
    a.partial_failure = false;
    assert!(!first.begin(512, 1, &mut a).is_null());
    first.complete();
    drop(first);
    drop(second);
    assert_eq!(a.state.borrow().live, 0);
    assert_eq!(b.state.borrow().live, 0);
    assert!(!a.state.borrow().released_during_compute);
}

#[test]
fn rust_owned_decode_cache_rejects_invalid_calls_without_build_or_eviction() {
    assert!(bonsai_qt_decode_cache_new(0).is_null());
    bonsai_qt_decode_cache_free(std::ptr::null_mut());
    let cache = Owner::new();
    let mut builder = builder();
    for (tokens, width) in [(0, 1), (-1, 1), (4097, 1), (256, 0), (256, -1)] {
        assert!(cache.begin(tokens, width, &mut builder).is_null());
    }
    assert_eq!(builder.state.borrow().before_build_live, []);
    assert!(!cache.begin(256, 1, &mut builder).is_null());
    cache.complete();
    let callbacks: [Option<super::decode_graph::Build>; 2] = [None, Some(build)];
    for callback in callbacks {
        assert!(
            bonsai_qt_decode_cache_begin(cache.0, 256, 1, std::ptr::null_mut(), callback, None)
                .is_null()
        );
    }
    assert_eq!(builder.state.borrow().live, 1);
    assert_eq!(builder.state.borrow().releases, 0);
    drop(cache);
    assert_eq!(builder.state.borrow().live, 0);
}
