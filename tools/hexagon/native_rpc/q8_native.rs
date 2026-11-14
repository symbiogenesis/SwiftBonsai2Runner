//! Native one-map Q8 adapter; dispatcher policy precedes every write/publication.

use super::{
    Driver, QueueBuffer, QueueRead, RemoteBuffer, WriteCall, cleanup_extent, compute, null_mut,
    open_module, write_packet,
};
use crate::{
    Error, Q8Buffer, Q8Completion, Q8Dispatcher, Q8Projection, Q8ProjectionReport, Q8Submitter,
    projection,
};
use std::ffi::c_void;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

fn entered(
    report: &mut Q8ProjectionReport,
    stage: &'static str,
    progress: &mut impl FnMut(&Q8ProjectionReport),
) {
    report.queue.stage = stage;
    progress(report);
}

fn active(
    report: &mut Q8ProjectionReport,
    cancellation: &AtomicBool,
    work_deadline: Instant,
) -> bool {
    report.cancelled |= cancellation.load(Ordering::Acquire);
    report.deadline_expired |= Instant::now() >= work_deadline;
    if report.cancelled || report.deadline_expired {
        report.queue.stage = if report.cancelled {
            "q8-cancelled-pinned"
        } else {
            "q8-work-expired-pinned"
        };
        return false;
    }
    true
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep native allocation, queue setup and terminal cleanup ownership linear for review"
)]
pub(super) fn run(
    driver: Driver,
    package: &Path,
    limits: Q8Projection<'_>,
    mut progress: impl FnMut(&Q8ProjectionReport),
    work: impl FnOnce(&mut dyn Q8Submitter) -> Result<(), Error>,
) -> Result<Q8ProjectionReport, Error> {
    // Revalidate the original selection/deadline before opening a native module.
    Q8Dispatcher::validate_selection(
        limits.plan,
        limits.tensor,
        limits.operations,
        limits.deadline,
        &limits.cancellation,
    )?;
    let work_deadline = limits
        .deadline
        .checked_sub(Duration::from_secs(3))
        .ok_or(Error::Contract)?;
    let mut report = Q8ProjectionReport::new(&limits);
    let (queue_report, opened) = open_module(
        driver,
        package,
        &mut |queue| {
            report.queue = *queue;
            progress(&report);
        },
        compute::ComputeFixture::Q8ProjectionNoBlock,
    )?;
    report.queue = queue_report;
    let Some((handle, domain, calls)) = opened else {
        return Ok(report);
    };
    if !active(&mut report, &limits.cancellation, work_deadline) {
        return Ok(report);
    }
    let allocation_bytes = limits.projection.allocation_bytes();
    let descriptor_size = u32::try_from(allocation_bytes).map_err(|_| Error::Contract)?;
    let allocator_request = i32::try_from(allocation_bytes).map_err(|_| Error::Contract)?;
    entered(&mut report, "q8-allocation-entered", &mut progress);
    // SAFETY: one bounded cached system-heap allocation, at most 64MiB and under i32 limits.
    let memory = unsafe { (calls.alloc)(25, 1, allocator_request) };
    if memory.is_null() || !memory.addr().is_multiple_of(4096) {
        report.queue.stage = "q8-allocation-refused-pinned";
        return Ok(report);
    }
    // SAFETY: fresh exclusive aligned extent, before any device consumer exists.
    unsafe {
        std::ptr::write_bytes(memory.cast::<u8>(), 0, allocation_bytes);
    }
    if !active(&mut report, &limits.cancellation, work_deadline) {
        return Ok(report);
    }
    // SAFETY: only this driver's fresh allocation is passed back to its own FD lookup.
    let fd = unsafe { (calls.to_fd)(memory) };
    let Ok(fd_u32) = u32::try_from(fd) else {
        report.queue.stage = "q8-fd-refused-pinned";
        return Ok(report);
    };
    let mapping = Q8Buffer::new(
        fd_u32,
        descriptor_size,
        0,
        0x90,
        u64::try_from(memory.addr()).map_err(|_| Error::Contract)?,
    )?;
    let dispatcher = Q8Dispatcher::new(
        limits.plan,
        limits.tensor,
        mapping,
        limits.operations,
        limits.deadline,
        Arc::clone(&limits.cancellation),
    )?;
    report.queue.allocation_succeeded = true;
    if !active(&mut report, &limits.cancellation, work_deadline) {
        return Ok(report);
    }
    entered(&mut report, "q8-fd-map-entered", &mut progress);
    // SAFETY: exact original effective domain, aligned owned pointer/FD/length, public FD=2 mode.
    let result = unsafe { (calls.map)(domain, fd, memory, 0, allocation_bytes, 2) };
    report.queue.mapping_result = Some(result);
    if result != 0 {
        report.queue.stage = "q8-fd-map-refused-pinned";
        return Ok(report);
    }
    if !active(&mut report, &limits.cancellation, work_deadline) {
        return Ok(report);
    }
    let mut queue = null_mut();
    entered(&mut report, "q8-queue-create-entered", &mut progress);
    // SAFETY: independently checked fixed eight-argument ABI, bounded queues, null callbacks/context.
    let result = unsafe {
        (calls.queue_create)(
            domain,
            0,
            4096,
            4096,
            None,
            None,
            null_mut(),
            &raw mut queue,
        )
    };
    report.queue.queue_create_result = Some(result);
    if result != 0 || queue.is_null() {
        report.queue.stage = "q8-queue-create-refused-pinned";
        return Ok(report);
    }
    if !active(&mut report, &limits.cancellation, work_deadline) {
        return Ok(report);
    }
    let mut queue_id = u64::MAX;
    entered(&mut report, "q8-queue-export-entered", &mut progress);
    // SAFETY: live driver-owned queue and writable poisoned 64-bit output.
    let result = unsafe { (calls.queue_export)(queue, &raw mut queue_id) };
    report.queue.queue_export_result = Some(result);
    if result != 0 || queue_id == u64::MAX {
        report.queue.stage = "q8-queue-export-refused-pinned";
        return Ok(report);
    }
    if !active(&mut report, &limits.cancellation, work_deadline) {
        return Ok(report);
    }
    let mut start = compute::start_wire(queue_id);
    start[32..40].copy_from_slice(&u64::from(descriptor_size).to_le_bytes());
    let mut argument = RemoteBuffer {
        pointer: start.as_mut_ptr().cast(),
        bytes: start.len(),
    };
    entered(&mut report, "q8-queue-start-entered", &mut progress);
    // SAFETY: checked seven-argument start wire; only the exact bounded allocation allowance varies.
    let result = unsafe { (calls.invoke)(handle, compute::START_SCALARS, &raw mut argument) };
    report.queue.queue_start_result = Some(result);
    if result != 0 {
        report.queue.stage = "q8-queue-start-refused-pinned";
        return Ok(report);
    }
    let expected = QueueBuffer {
        fd: fd_u32,
        size: descriptor_size,
        offset: 0,
        flags: 0x90,
        pointer: memory,
    };
    let (work_ok, terminal) = {
        let mut session = Session {
            report: &mut report,
            queue_write: &calls.queue_write,
            queue_read: calls.queue_read,
            queue,
            memory,
            expected,
            dispatcher,
            work_deadline,
            output_values: limits.projection.rows() * limits.projection.batch(),
            failed_or_active: false,
            cancellation: Arc::clone(&limits.cancellation),
            progress: &mut progress,
        };
        let result = work(&mut session);
        (
            result.is_ok(),
            session.failed_or_active || session.dispatcher.is_terminal(),
        )
    };
    if !work_ok
        || terminal
        || report.operations_completed == 0
        || report.operations_submitted != report.operations_completed
    {
        if !terminal {
            report.queue.stage = "q8-caller-rejected-pinned";
        }
        return Ok(report);
    }
    if !active(&mut report, &limits.cancellation, work_deadline) {
        return Ok(report);
    }
    report.work_completed = true;
    report.queue = cleanup_extent(
        report.queue,
        &calls.cleanup_calls(),
        handle,
        queue,
        domain,
        fd,
        memory,
        allocation_bytes,
        &mut |queue| {
            let mut snapshot = report;
            snapshot.queue = *queue;
            progress(&snapshot);
        },
    );
    report.deadline_expired |= Instant::now() >= limits.deadline;
    report.cancelled |= limits.cancellation.load(Ordering::Acquire);
    progress(&report);
    Ok(report)
}

struct Session<'a, 'plan, P> {
    report: &'a mut Q8ProjectionReport,
    queue_write: &'a WriteCall,
    queue_read: QueueRead,
    queue: *mut c_void,
    memory: *mut c_void,
    expected: QueueBuffer,
    dispatcher: Q8Dispatcher<'plan>,
    output_values: usize,
    // Set before any caller progress/native entry; a caught unwind cannot
    // reopen a mapping with an uncertain consumer or authorize normal cleanup.
    failed_or_active: bool,
    work_deadline: Instant,
    cancellation: Arc<AtomicBool>,
    progress: &'a mut P,
}

struct Reply {
    flags: u32,
    count: u32,
    buffer: Q8Buffer,
    response: [u8; compute::COMPUTE_RESPONSE_BYTES],
}

impl<P: FnMut(&Q8ProjectionReport)> Session<'_, '_, P> {
    fn fail(&mut self, stage: &'static str) -> Error {
        self.dispatcher.abandon();
        self.failed_or_active = true;
        self.report.queue.poisoned = true;
        self.report.queue.stage = stage;
        (self.progress)(self.report);
        Error::Contract
    }

    fn active(&mut self) -> bool {
        if active(self.report, &self.cancellation, self.work_deadline) {
            return true;
        }
        self.dispatcher.abandon();
        self.report.queue.poisoned = true;
        (self.progress)(self.report);
        false
    }

    fn read(&mut self, sequence: u64) -> Option<Reply> {
        let deadline = (Instant::now() + Duration::from_secs(3)).min(self.work_deadline);
        loop {
            if !self.active() {
                return None;
            }
            if Instant::now() >= deadline {
                let _ = self.fail("q8-read-expired-pinned");
                return None;
            }
            let mut flags = u32::MAX;
            let mut count = u32::MAX;
            let mut bytes = u32::MAX;
            let mut descriptor = QueueBuffer {
                fd: u32::MAX,
                size: u32::MAX,
                offset: u32::MAX,
                flags: u32::MAX,
                pointer: null_mut(),
            };
            let mut response = [0xa5; compute::COMPUTE_RESPONSE_BYTES];
            // SAFETY: checked no-block ABI; every poisoned output has its declared full capacity.
            let result = unsafe {
                (self.queue_read)(
                    self.queue,
                    &raw mut flags,
                    1,
                    &raw mut count,
                    &raw mut descriptor,
                    88,
                    &raw mut bytes,
                    response.as_mut_ptr(),
                )
            };
            self.report.queue.queue_read_result = Some(result);
            if result == 516 {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            // Validate correlation before constructing any slice of asynchronously used memory.
            self.report.queue.response_validated = result == 0
                && count == 1
                && bytes == 88
                && flags & 3 == 3
                && flags & !0x1f == 0
                && descriptor.fd == self.expected.fd
                && descriptor.size == self.expected.size
                && descriptor.offset == self.expected.offset
                && descriptor.flags == self.expected.flags
                && descriptor.pointer == self.expected.pointer
                && projection::response_valid(&response, sequence);
            if !self.report.queue.response_validated {
                let _ = self.fail("q8-response-refused-pinned");
                return None;
            }
            let Ok(address) = u64::try_from(descriptor.pointer.addr()) else {
                let _ = self.fail("q8-response-address-refused-pinned");
                return None;
            };
            let Ok(buffer) = Q8Buffer::new(
                descriptor.fd,
                descriptor.size,
                descriptor.offset,
                descriptor.flags,
                address,
            ) else {
                let _ = self.fail("q8-response-descriptor-refused-pinned");
                return None;
            };
            return Some(Reply {
                flags,
                count,
                buffer,
                response,
            });
        }
    }
}

impl<P: FnMut(&Q8ProjectionReport)> Q8Submitter for Session<'_, '_, P> {
    fn submit(
        &mut self,
        weights: &[u8],
        activation: &[f32],
        output: &mut [f32],
        oracle: &mut dyn FnMut(&[f32]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if !self.active() || self.failed_or_active || self.dispatcher.is_terminal() {
            return Err(self.fail("q8-session-refused-pinned"));
        }
        self.failed_or_active = true;
        if output.len() != self.output_values {
            return Err(self.fail("q8-output-shape-refused-pinned"));
        }
        let allocation = usize::try_from(self.expected.size)
            .map_err(|_| self.fail("q8-extent-refused-pinned"))?;
        // SAFETY: no prior pending consumer exists: the first write has not occurred,
        // or dispatcher accepted the previous correlated completion and cache flags.
        // This unique view ends before entering the native write.
        let image = unsafe { std::slice::from_raw_parts_mut(self.memory.cast::<u8>(), allocation) };
        let request = self
            .dispatcher
            .prepare(image, weights, activation)
            .map_err(|_| self.fail("q8-input-refused-pinned"))?;
        self.report.queue.response_validated = false;
        self.report.queue.numeric_validated = false;
        if !self.active() {
            return Err(Error::Contract);
        }
        let mut descriptor = self.expected;
        entered(self.report, "q8-queue-write-entered", self.progress);
        if !self.active() {
            return Err(Error::Contract);
        }
        self.report.operations_submitted += 1;
        self.report.queue.compute_submitted = true;
        let result = write_packet(
            self.queue_write,
            self.queue,
            &mut descriptor,
            request.wire(),
        );
        self.report.queue.queue_write_result = Some(result);
        if let Err(error) = self.dispatcher.record_write(result) {
            let _ = self.active();
            let _ = self.fail("q8-write-refused-pinned");
            return Err(error);
        }
        let sequence = u64::from_le_bytes(
            request.wire()[..8]
                .try_into()
                .map_err(|_| self.fail("q8-sequence-refused-pinned"))?,
        );
        entered(self.report, "q8-queue-read-entered", self.progress);
        let reply = self.read(sequence).ok_or(Error::Contract)?;
        // SAFETY: read validated the original write's sequence, exact allocation
        // descriptor/pointer/count, complete response and global cache flags before
        // any view was constructed. The native consumer completed; no mutable view
        // survives preparation, and no caller ever receives this mapping.
        let image = unsafe { std::slice::from_raw_parts(self.memory.cast::<u8>(), allocation) };
        let completion = Q8Completion {
            read_result: 0,
            flags: reply.flags,
            buffer_count: reply.count,
            buffer: reply.buffer,
            response: &reply.response,
            image,
        };
        if let Err(error) = self.dispatcher.complete(completion, output, oracle) {
            let _ = self.active();
            let _ = self.fail("q8-output-refused-pinned");
            return Err(error);
        }
        self.report.operations_completed = self.dispatcher.completed();
        self.report.queue.numeric_validated = true;
        entered(self.report, "q8-operation-complete", self.progress);
        self.failed_or_active = false;
        Ok(())
    }
}

#[cfg(test)]
#[path = "q8_native_tests.rs"]
mod tests;
