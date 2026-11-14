use super::*;
use crate::{CacheOwner, DispatchIdentity, HybridPlan, Q8Matmul, TensorPlacement};
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    Write,
    Read,
    Stale,
    Descriptor,
    Extent,
    Numeric,
    Oracle,
    CancelRead,
    CancelOracle,
    OracleUnwind,
    Count,
    Flags,
    OutputShape,
    Budget,
    ProgressUnwind,
    Expired,
}

struct Mock {
    writes: u32,
    reads: u32,
    sequence: u64,
    descriptor: [u32; 4],
    pointer: usize,
    output: usize,
    fault: Fault,
    cancellation: Option<Arc<AtomicBool>>,
}

static SERIAL: Mutex<()> = Mutex::new(());
static MOCK: Mutex<Mock> = Mutex::new(Mock {
    writes: 0,
    reads: 0,
    sequence: 0,
    descriptor: [0; 4],
    pointer: 0,
    output: 0,
    fault: Fault::None,
    cancellation: None,
});

unsafe extern "C" fn write(
    _queue: *mut c_void,
    _flags: u32,
    count: u32,
    descriptor: *mut QueueBuffer,
    bytes: u32,
    request: *const u8,
) -> i32 {
    let Ok(mut state) = MOCK.lock() else {
        return -77;
    };
    if count != 1 || bytes != 24 {
        return -78;
    }
    state.writes += 1;
    // SAFETY: only the actual adapter calls this test mock, with one complete
    // live descriptor and the exact 24-byte request retained for this call.
    let descriptor = unsafe { *descriptor };
    // SAFETY: the safe adapter supplies a live request of exactly the checked size.
    let request = unsafe { std::slice::from_raw_parts(request, 24) };
    let Ok(sequence) = request[..8].try_into() else {
        return -79;
    };
    state.sequence = u64::from_le_bytes(sequence);
    state.descriptor = [
        descriptor.fd,
        descriptor.size,
        descriptor.offset,
        descriptor.flags,
    ];
    state.pointer = descriptor.pointer.expose_provenance();
    if state.fault == Fault::Write {
        return 14;
    }
    let Ok(allocation) = usize::try_from(descriptor.size) else {
        return -80;
    };
    // SAFETY: this exact allocation is owned by the test; the adapter ended its
    // unique preparation borrow before entering the synchronous mock write.
    let image =
        unsafe { std::slice::from_raw_parts_mut(descriptor.pointer.cast::<u8>(), allocation) };
    for slot in image[state.output..state.output + 256]
        .as_chunks_mut::<4>()
        .0
    {
        *slot = 7.0_f32.to_le_bytes();
    }
    if state.fault == Fault::Numeric {
        image[state.output + 252..state.output + 256].copy_from_slice(&f32::NAN.to_le_bytes());
    }
    0
}

unsafe extern "C" fn read(
    _queue: *mut c_void,
    flags: *mut u32,
    _capacity: u32,
    count: *mut u32,
    descriptor: *mut QueueBuffer,
    _maximum: u32,
    bytes: *mut u32,
    response: *mut u8,
) -> i32 {
    let Ok(mut state) = MOCK.lock() else {
        return -77;
    };
    state.reads += 1;
    if state.fault == Fault::Read {
        return 19;
    }
    if state.fault == Fault::CancelRead {
        if let Some(cancellation) = &state.cancellation {
            cancellation.store(true, Ordering::Release);
        }
        return 516;
    }
    let [fd, size, offset, cache] = state.descriptor;
    // Literal source-bound response fields: the mock is a transport simulation,
    // never a DSP Q8 arithmetic result or a fresh native-engine observation.
    let mut wire = [0; 88];
    let sequence = state.sequence - u64::from(state.fault == Fault::Stale);
    wire[..8].copy_from_slice(&sequence.to_le_bytes());
    for (offset, value) in [(24, 1_u32), (28, 1), (32, 3), (36, 1)] {
        wire[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    // SAFETY: adapter supplies a complete writable u32 flags slot.
    unsafe {
        *flags = if state.fault == Fault::Flags { 2 } else { 3 };
    }
    // SAFETY: adapter supplies a complete writable u32 descriptor-count slot.
    unsafe {
        *count = if state.fault == Fault::Count { 2 } else { 1 };
    }
    // SAFETY: adapter supplies a complete writable u32 response-length slot.
    unsafe {
        *bytes = if state.fault == Fault::Extent { 87 } else { 88 };
    }
    // SAFETY: adapter supplies a complete writable descriptor; pointer provenance
    // was recorded from the same original test allocation during the mock write.
    unsafe {
        *descriptor = QueueBuffer {
            fd: fd + u32::from(state.fault == Fault::Descriptor),
            size,
            offset,
            flags: cache,
            pointer: std::ptr::with_exposed_provenance_mut(state.pointer),
        };
    }
    // SAFETY: adapter supplies a separate writable response with full 88-byte capacity.
    unsafe {
        std::ptr::copy_nonoverlapping(wire.as_ptr(), response, 88);
    }
    0
}

#[derive(Clone)]
#[repr(C, align(4096))]
struct Page([u8; 4096]);

fn plan(projection: Q8Matmul) -> Result<HybridPlan, Error> {
    HybridPlan::new(
        DispatchIdentity {
            model: [1; 32],
            model_bytes: 100_000,
            module: [2; 32],
            catalog: [3; 32],
            driver: [4; 32],
            source: [5; 20],
        },
        &[TensorPlacement::Htp0Q8 {
            tensor: 2,
            offset: 32,
            projection,
        }],
        CacheOwner::Cpu,
    )
}

#[test]
fn q8_native_adapter_reuses_only_complete_independently_accepted_results() -> Result<(), Error> {
    let _serial = SERIAL.lock().map_err(|_| Error::Contract)?;
    let projection = Q8Matmul::new(128, 32, 2, 100_000)?;
    let plan = plan(projection)?;
    for fault in [
        Fault::None,
        Fault::Write,
        Fault::Read,
        Fault::Stale,
        Fault::Descriptor,
        Fault::Extent,
        Fault::Numeric,
        Fault::Oracle,
        Fault::CancelRead,
        Fault::CancelOracle,
        Fault::OracleUnwind,
        Fault::Count,
        Fault::Flags,
        Fault::OutputShape,
        Fault::Budget,
        Fault::ProgressUnwind,
        Fault::Expired,
    ] {
        let cancellation = Arc::new(AtomicBool::new(false));
        *MOCK.lock().map_err(|_| Error::Contract)? = Mock {
            writes: 0,
            reads: 0,
            sequence: 0,
            descriptor: [0; 4],
            pointer: 0,
            output: projection.output_offset(),
            fault,
            cancellation: Some(Arc::clone(&cancellation)),
        };
        let mut pages = vec![Page([0; 4096]); projection.allocation_bytes() / 4096];
        let memory = pages.as_mut_ptr().cast::<c_void>();
        let size = u32::try_from(projection.allocation_bytes()).map_err(|_| Error::Contract)?;
        let mapping = Q8Buffer::new(
            7,
            size,
            0,
            0x90,
            u64::try_from(memory.addr()).map_err(|_| Error::Contract)?,
        )?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let dispatcher = Q8Dispatcher::new(
            &plan,
            2,
            mapping,
            if fault == Fault::Budget { 1 } else { 2 },
            deadline,
            Arc::clone(&cancellation),
        )?;
        let expected = QueueBuffer {
            fd: 7,
            size,
            offset: 0,
            flags: 0x90,
            pointer: memory,
        };
        let limits = Q8Projection::new(
            &plan,
            2,
            if fault == Fault::Budget { 1 } else { 2 },
            deadline,
            Arc::clone(&cancellation),
        )?;
        let mut report = Q8ProjectionReport::new(&limits);
        report.queue.poisoned = true;
        let write = WriteCall::Nonblocking(write);
        let mut progress = |report: &Q8ProjectionReport| {
            if fault == Fault::ProgressUnwind && report.queue.stage == "q8-queue-read-entered" {
                std::panic::resume_unwind(Box::new("progress-test-unwind"));
            }
        };
        let mut session = Session {
            report: &mut report,
            queue_write: &write,
            queue_read: read,
            queue: null_mut(),
            memory,
            expected,
            dispatcher,
            output_values: 64,
            failed_or_active: false,
            work_deadline: deadline - Duration::from_secs(3),
            cancellation: Arc::clone(&cancellation),
            progress: &mut progress,
        };
        if fault == Fault::Expired {
            session.work_deadline = Instant::now();
        }
        let weights = vec![0; projection.weight_bytes()];
        let mut output = [99.0; 64];
        let mut oracle_calls = 0;
        let mut oracle = |values: &[f32]| {
            oracle_calls += 1;
            if fault == Fault::OracleUnwind {
                std::panic::resume_unwind(Box::new("oracle-test-unwind"));
            }
            if fault == Fault::CancelOracle {
                cancellation.store(true, Ordering::Release);
            }
            if values == [7.0; 64] && fault != Fault::Oracle {
                Ok(())
            } else {
                Err(Error::Contract)
            }
        };
        let output_slice = if fault == Fault::OutputShape {
            &mut output[..63]
        } else {
            &mut output[..]
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.submit(&weights, &[0.0; 256], output_slice, &mut oracle)
        }));
        if matches!(fault, Fault::None | Fault::Budget) {
            assert!(result.is_ok_and(|result| result.is_ok()));
            assert_eq!(output, [7.0; 64]);
            assert_eq!(session.report.operations_completed, 1);
            let second = session.submit(&weights, &[0.0; 256], &mut output, &mut |_| Ok(()));
            assert_eq!(second.is_ok(), fault == Fault::None);
            assert_eq!(
                session.report.operations_completed,
                if fault == Fault::None { 2 } else { 1 }
            );
        } else {
            assert!(result.is_err() || result.is_ok_and(|result| result.is_err()));
            assert_eq!(output, [99.0; 64]);
            assert_eq!(session.report.operations_completed, 0);
            assert!(session.failed_or_active || session.dispatcher.is_terminal());
        }
        let before = MOCK.lock().map_err(|_| Error::Contract)?.writes;
        assert!(
            session
                .submit(&weights, &[0.0; 256], &mut output, &mut |_| Ok(()))
                .is_err()
        );
        assert_eq!(MOCK.lock().map_err(|_| Error::Contract)?.writes, before);
        assert_eq!(session.report.operations_submitted, before);
        assert!(!session.report.completed_cycle());
        if matches!(fault, Fault::Write | Fault::OutputShape | Fault::Expired) {
            assert_eq!(MOCK.lock().map_err(|_| Error::Contract)?.reads, 0);
            assert_eq!(oracle_calls, 0);
        }
        if fault == Fault::Expired {
            assert!(session.report.deadline_expired);
        }
        if matches!(fault, Fault::CancelRead | Fault::CancelOracle) {
            assert!(session.report.cancelled);
        }
        assert!(!session.report.queue.free_completed);
        assert!(session.report.queue.queue_stop_result.is_none());
    }
    Ok(())
}

#[test]
fn q8_native_selection_rejects_bad_bounds_before_any_transport() -> Result<(), Error> {
    let projection = Q8Matmul::new(128, 32, 2, 100_000)?;
    let plan = plan(projection)?;
    let cancellation = Arc::new(AtomicBool::new(false));
    for (tensor, operations, seconds) in
        [(1, 2, 5), (2, 0, 5), (2, 24_001, 5), (2, 2, 3), (2, 2, 121)]
    {
        assert!(
            Q8Projection::new(
                &plan,
                tensor,
                operations,
                Instant::now() + Duration::from_secs(seconds),
                Arc::clone(&cancellation)
            )
            .is_err()
        );
    }
    cancellation.store(true, Ordering::Release);
    assert!(
        Q8Projection::new(
            &plan,
            2,
            2,
            Instant::now() + Duration::from_secs(5),
            cancellation
        )
        .is_err()
    );
    Ok(())
}
