//! Bounded, reusable projection workers with synchronous borrowed row jobs.
#![forbid(unsafe_code)]

use std::{cell::RefCell, thread::JoinHandle};

use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

thread_local! {
    // The serving thread owns its team through voice/model switches. Dropping
    // that thread or changing its parallel team size releases the old workers.
    static TEAM: RefCell<Option<CachedTeam>> = const { RefCell::new(None) };
}

enum CachedTeam {
    Ready(OwnedTeam),
    Unavailable(usize),
}

impl CachedTeam {
    const fn worker_count(&self) -> usize {
        match self {
            Self::Ready(team) => team.workers.len(),
            Self::Unavailable(count) => *count,
        }
    }
}

struct OwnedTeam {
    pool: Option<ThreadPool>,
    workers: Vec<JoinHandle<()>>,
}

impl OwnedTeam {
    fn build(worker_count: usize) -> Result<Self, rayon::ThreadPoolBuildError> {
        let mut team = Self {
            pool: None,
            workers: Vec::with_capacity(worker_count),
        };
        let pool = ThreadPoolBuilder::new()
            .num_threads(worker_count)
            .spawn_handler(|worker| {
                let handle = std::thread::Builder::new()
                    .name(format!("tts-row-{}", worker.index()))
                    .spawn(move || worker.run())?;
                team.workers.push(handle);
                Ok(())
            })
            .build()?;
        team.pool = Some(pool);
        Ok(team)
    }
}

impl Drop for OwnedTeam {
    fn drop(&mut self) {
        // Rayon signals termination when the pool drops. Join even a partial
        // construction before leaving or creating a replacement team.
        drop(self.pool.take());
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

pub(super) fn run<F>(out: &mut [f32], worker_count: usize, compute_slice: &F)
where
    F: Fn(usize, &mut [f32]) + Sync,
{
    let worker_count = worker_count.clamp(1, 18).min(out.len().max(1));
    if worker_count <= 1 || out.len() < 4 || rayon::current_thread_index().is_some() {
        compute_slice(0, out);
        return;
    }

    let completed = TEAM.with(|slot| {
        let Ok(mut team) = slot.try_borrow_mut() else {
            return false;
        };
        if team
            .as_ref()
            .is_none_or(|team| team.worker_count() != worker_count)
        {
            // Release the old team before creating another, retaining the
            // selected bound even when a caller changes its worker count.
            *team = None;
            *team = Some(
                OwnedTeam::build(worker_count)
                    .map_or_else(|_| CachedTeam::Unavailable(worker_count), CachedTeam::Ready),
            );
        }
        let Some(CachedTeam::Ready(team)) = team.as_ref() else {
            return false;
        };
        let Some(pool) = team.pool.as_ref() else {
            return false;
        };
        let chunk_rows = out.len().div_ceil(worker_count);
        pool.install(|| {
            out.par_chunks_mut(chunk_rows)
                .enumerate()
                .for_each(|(chunk, rows)| compute_slice(chunk * chunk_rows, rows));
        });
        true
    });
    if !completed {
        // Thread creation failure never removes speech availability, and no
        // partial job has been submitted before taking this serial fallback.
        compute_slice(0, out);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        io,
        sync::{Barrier, Mutex},
        thread::{self, ThreadId},
    };

    use super::{CachedTeam, OwnedTeam, TEAM, run};

    #[test]
    fn projection_workers_reuse_threads_and_cover_uneven_rows() -> io::Result<()> {
        fn team_ids() -> io::Result<HashSet<ThreadId>> {
            let barrier = Barrier::new(4);
            let ids = Mutex::new(HashSet::new());
            let mut rows = [0.0; 16];
            run(&mut rows, 4, &|_, output| {
                ids.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(thread::current().id());
                barrier.wait();
                output.fill(1.0);
            });
            assert_eq!(rows, [1.0; 16]);
            ids.into_inner()
                .map_err(|_| io::Error::other("Thread IDs poisoned."))
        }

        // Return construction failure before entering a barrier: the public
        // helper's serial fallback must never make this control hang.
        let team = OwnedTeam::build(4).map_err(io::Error::other)?;
        TEAM.with(|slot| *slot.borrow_mut() = Some(CachedTeam::Ready(team)));
        assert!(rayon::current_thread_index().is_none());
        let first = team_ids()?;
        assert_eq!(first.len(), 4);
        assert_eq!(first, team_ids()?);
        let mut rows = [0.0; 17];
        run(&mut rows, 4, &|offset, output| {
            for (index, value) in output.iter_mut().enumerate() {
                *value = f32::from(u16::try_from(offset + index).unwrap_or(0));
            }
        });
        assert_eq!(
            rows,
            std::array::from_fn(|index| { f32::from(u16::try_from(index).unwrap_or(0)) })
        );
        Ok(())
    }

    #[test]
    fn projection_workers_handle_nested_and_serial_jobs() {
        let mut rows = [0.0; 9];
        run(&mut rows, 3, &|offset, output| {
            let mut nested = [0.0; 5];
            run(&mut nested, 3, &|start, nested_output| {
                assert_eq!(start, 0);
                nested_output.fill(2.0);
            });
            assert_eq!(nested, [2.0; 5]);
            for (index, value) in output.iter_mut().enumerate() {
                *value = f32::from(u16::try_from(offset + index).unwrap_or(0));
            }
        });
        assert_eq!(rows, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        run(&mut rows, 1, &|offset, output| {
            assert_eq!(offset, 0);
            output.fill(3.0);
        });
        assert_eq!(rows, [3.0; 9]);

        TEAM.with(|slot| *slot.borrow_mut() = Some(CachedTeam::Unavailable(3)));
        run(&mut rows, 3, &|offset, output| {
            assert_eq!(offset, 0);
            assert_eq!(output.len(), 9);
            output.fill(4.0);
        });
        assert_eq!(rows, [4.0; 9]);
        TEAM.with(|slot| {
            assert!(matches!(*slot.borrow(), Some(CachedTeam::Unavailable(3))));
        });
    }
}
