//! Host-side Q8 queue transactions, independent of native execution authority.

use crate::dispatch::{BufferIdentity, SubmissionTracker};
use crate::{Error, HybridPlan, Q8Matmul, TensorPlacement, compute, projection};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// One exact mapped queue buffer, including cache-maintenance flags.
///
/// This value records the transport's observations; it neither allocates nor
/// maps memory. Creating it does not authenticate a driver or signed package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Q8Buffer(BufferIdentity);

impl Q8Buffer {
    /// Validate the fixed FD=2 transport's complete buffer descriptor.
    ///
    /// # Errors
    /// Rejects negative FDs, nonzero offsets, different cache flags, unaligned
    /// addresses/extents, zero extents and overflowing address ranges.
    pub fn new(fd: u32, bytes: u32, offset: u32, flags: u32, address: u64) -> Result<Self, Error> {
        if fd > i32::MAX.unsigned_abs()
            || bytes == 0
            || !bytes.is_multiple_of(4096)
            || offset != 0
            || flags != 0x90
            || address == 0
            || !address.is_multiple_of(4096)
            || address.checked_add(u64::from(bytes)).is_none()
        {
            return Err(Error::Contract);
        }
        Ok(Self(BufferIdentity {
            fd,
            bytes,
            offset,
            flags,
            address,
        }))
    }

    /// Driver-owned FD identifying the complete mapping.
    #[must_use]
    pub const fn fd(self) -> u32 {
        self.0.fd
    }
    /// Complete mapping size, including command, weights, inputs and output.
    #[must_use]
    pub const fn bytes(self) -> u32 {
        self.0.bytes
    }
    /// Exact mapping offset, fixed to zero.
    #[must_use]
    pub const fn offset(self) -> u32 {
        self.0.offset
    }
    /// Source-defined cache-maintenance flags, fixed to 0x90.
    #[must_use]
    pub const fn flags(self) -> u32 {
        self.0.flags
    }
    /// Exact observed mapping address; never dereferenced by this module.
    #[must_use]
    pub const fn address(self) -> u64 {
        self.0.address
    }
}

/// One prepared request whose sequence is consumed before a native write.
#[derive(Clone, Copy, Debug)]
pub struct Q8Request {
    wire: [u8; compute::COMPUTE_REQUEST_BYTES],
    buffer: Q8Buffer,
}

impl Q8Request {
    /// Exact request bytes for one write; no retry is permitted.
    #[must_use]
    pub const fn wire(&self) -> &[u8; compute::COMPUTE_REQUEST_BYTES] {
        &self.wire
    }
    /// The descriptor that must accompany the request and correlated completion.
    #[must_use]
    pub const fn buffer(self) -> Q8Buffer {
        self.buffer
    }
}

/// Raw observations from exactly one native read after a successful write.
///
/// The transport must finish the native read and cache invalidation before
/// borrowing `image`. These observations are checked, not authenticated here.
#[derive(Clone, Copy)]
pub struct Q8Completion<'a> {
    /// Unmodified native read result; any nonzero result is terminal.
    pub read_result: i32,
    /// Exact returned queue flags, including buffer/data availability.
    pub flags: u32,
    /// Exact returned descriptor count, required to be one.
    pub buffer_count: u32,
    /// Exact returned descriptor, including pointer and cache flags.
    pub buffer: Q8Buffer,
    /// Complete returned response bytes; truncated/extra bytes reject.
    pub response: &'a [u8],
    /// Complete mapped image after the correlated read and cache invalidation.
    pub image: &'a [u8],
}

/// One immutable-placement, single-flight Q8 host dispatcher.
///
/// The caller owns an independently admitted native transport, guarded files,
/// a contained child and verified teardown. This module opens no native session
/// and grants no execution authority. It never frees mapped memory, retries a
/// write, falls back to CPU, changes placement or resets an operation budget.
/// Uncertainty, cancellation and unwinding during acceptance are terminal.
pub struct Q8Dispatcher<'a> {
    plan: &'a HybridPlan,
    projection: Q8Matmul,
    mapping: Q8Buffer,
    submissions: SubmissionTracker,
    work_deadline: Instant,
    cancellation: Arc<AtomicBool>,
    terminal: bool,
    pending: Option<u64>,
    write_succeeded: bool,
    attempted: u32,
    completed: u32,
    scratch: Vec<f32>,
}

impl<'a> Q8Dispatcher<'a> {
    /// Bind one selected HTP0 tensor to a fixed mapping and inherited deadline.
    ///
    /// Construct before native setup, using the original session's absolute
    /// deadline. At most 24,000 preparations and 120 seconds are admitted, with
    /// three seconds reserved inside that deadline for external cleanup. The
    /// shared cancellation flag is observed with acquire ordering at preparation
    /// and before publication; native calls still require external supervision.
    ///
    /// # Errors
    /// Rejects CPU/unselected tensors, wrong mapping extents, cancellation,
    /// expired/excessive allowances, empty/excessive work or unavailable scratch.
    pub fn new(
        plan: &'a HybridPlan,
        tensor: u16,
        mapping: Q8Buffer,
        operations: u32,
        deadline: Instant,
        cancellation: Arc<AtomicBool>,
    ) -> Result<Self, Error> {
        let now = Instant::now();
        let remaining = deadline
            .checked_duration_since(now)
            .ok_or(Error::Contract)?;
        if operations == 0
            || operations > 24_000
            || remaining <= Duration::from_secs(3)
            || remaining > Duration::from_secs(120)
            || cancellation.load(Ordering::Acquire)
        {
            return Err(Error::Contract);
        }
        let projection = plan
            .placements()
            .iter()
            .find_map(|placement| match *placement {
                TensorPlacement::Htp0Q8 {
                    tensor: selected,
                    projection,
                    ..
                } if selected == tensor => Some(projection),
                _ => None,
            })
            .ok_or(Error::Contract)?;
        if usize::try_from(mapping.bytes()).map_err(|_| Error::Contract)?
            != projection.allocation_bytes()
        {
            return Err(Error::Contract);
        }
        let mut scratch = Vec::new();
        let count = projection.rows() * projection.batch();
        scratch
            .try_reserve_exact(count)
            .map_err(|_| Error::Contract)?;
        scratch.resize(count, 0.0);
        Ok(Self {
            plan,
            projection,
            mapping,
            submissions: SubmissionTracker::new(mapping.0, operations),
            work_deadline: deadline
                .checked_sub(Duration::from_secs(3))
                .ok_or(Error::Contract)?,
            cancellation,
            terminal: false,
            pending: None,
            write_succeeded: false,
            attempted: 0,
            completed: 0,
            scratch,
        })
    }

    /// Original immutable placement/source/model identity bound by this owner.
    #[must_use]
    pub const fn plan(&self) -> &HybridPlan {
        self.plan
    }
    /// Preparations consumed, including refused or uncertain write attempts.
    #[must_use]
    pub const fn attempted(&self) -> u32 {
        self.attempted
    }
    /// Complete correlated and independently accepted outputs published.
    #[must_use]
    pub const fn completed(&self) -> u32 {
        self.completed
    }
    /// Whether a failure permanently prohibits further submissions.
    ///
    /// A false value establishes neither native cleanup nor permission to free.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        self.terminal
    }

    fn active(&self) -> bool {
        !self.terminal
            && !self.cancellation.load(Ordering::Acquire)
            && Instant::now() < self.work_deadline
    }

    const fn reject(&mut self) -> Error {
        self.terminal = true;
        self.submissions.poison();
        Error::Contract
    }

    /// Prepare one command and consume its sequence before entering the write.
    ///
    /// All input validation precedes image mutation. The transport must preserve
    /// every image byte after this returns until correlated completion. Report
    /// the write result exactly once; do not write the request a second time.
    ///
    /// # Errors
    /// Rejects outstanding work, exhausted bounds, cancellation, invalid source
    /// tensors/activations or expired work. Any rejection is terminal.
    pub fn prepare(
        &mut self,
        image: &mut [u8],
        weights: &[u8],
        activation: &[f32],
    ) -> Result<Q8Request, Error> {
        if !self.active() || !self.submissions.reusable() || self.pending.is_some() {
            return Err(self.reject());
        }
        // Remain terminal if serialization ever unwinds.
        self.terminal = true;
        self.projection.write_image(
            image,
            self.mapping.address(),
            self.mapping.fd(),
            weights,
            activation,
        )?;
        if self.cancellation.load(Ordering::Acquire) || Instant::now() >= self.work_deadline {
            return Err(self.reject());
        }
        let sequence = self.submissions.begin()?;
        self.pending = Some(sequence);
        self.write_succeeded = false;
        self.attempted += 1;
        self.terminal = false;
        Ok(Q8Request {
            wire: projection::request(sequence),
            buffer: self.mapping,
        })
    }

    /// Record the original native write result exactly once, without retry.
    ///
    /// # Errors
    /// A refused write stays terminal even if a later valid reply arrives.
    /// Duplicate or out-of-order calls, cancellation and expiry also reject.
    pub fn record_write(&mut self, result: i32) -> Result<(), Error> {
        if !self.active() || self.pending.is_none() || self.write_succeeded {
            return Err(self.reject());
        }
        if result != 0 {
            let _ = self.reject();
            return Err(Error::Native("q8-queue-write", i64::from(result)));
        }
        self.write_succeeded = true;
        Ok(())
    }

    /// Validate and publish a complete response after independent numerical review.
    ///
    /// The oracle must accept every output using the unchanged source inputs;
    /// finite values alone cannot establish Q8 arithmetic. Output is unchanged
    /// unless the entire response and oracle succeed before the work deadline.
    /// A panicking oracle leaves the owner terminal and publishes no output.
    ///
    /// # Errors
    /// Rejects failed reads/writes, stale sequences, changed mappings/cache flags,
    /// wrong wire extents/counts, nonfinite output, oracle refusal, cancellation
    /// and expiry. No response after refusal permits another write or cleanup.
    pub fn complete(
        &mut self,
        completion: Q8Completion<'_>,
        output: &mut [f32],
        oracle: impl FnOnce(&[f32]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if !self.active() || !self.write_succeeded {
            return Err(self.reject());
        }
        // Every early return or unwind from the independent oracle is terminal.
        self.terminal = true;
        let sequence = self.pending.ok_or(Error::Contract)?;
        if completion.read_result != 0 {
            self.submissions.poison();
            return Err(Error::Native(
                "q8-queue-read",
                i64::from(completion.read_result),
            ));
        }
        if completion.flags & 3 != 3
            || completion.flags & !0x1f != 0
            || completion.buffer_count != 1
            || completion.buffer != self.mapping
            || !projection::response_valid(completion.response, sequence)
            || completion.image.len() != self.projection.allocation_bytes()
            || output.len() != self.scratch.len()
        {
            return Err(self.reject());
        }
        let begin = self.projection.output_offset();
        let bytes = &completion.image[begin..begin + self.scratch.len() * 4];
        for (value, encoded) in self.scratch.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *value = f32::from_le_bytes(*encoded);
            if !value.is_finite() {
                return Err(self.reject());
            }
        }
        oracle(&self.scratch)?;
        if self.cancellation.load(Ordering::Acquire) || Instant::now() >= self.work_deadline {
            return Err(self.reject());
        }
        self.submissions.complete(sequence, completion.buffer.0)?;
        output.copy_from_slice(&self.scratch);
        self.completed += 1;
        self.pending = None;
        self.write_succeeded = false;
        self.terminal = false;
        Ok(())
    }

    /// Permanently stop this owner after cancellation or uncertain transport work.
    ///
    /// This does not acknowledge DSP completion or release any mapped/native owner.
    pub const fn abandon(&mut self) {
        self.terminal = true;
        self.submissions.poison();
    }
}

#[cfg(test)]
#[path = "q8_dispatch_tests.rs"]
mod tests;
