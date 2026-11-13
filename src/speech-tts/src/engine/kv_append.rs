//! Transactional KV publication for one decoder position, including unwinding.
#![forbid(unsafe_code)]

use std::io;

use super::LayerKvCache;

pub(super) struct KvAppend<'a> {
    cache: &'a mut LayerKvCache,
    keys: usize,
    values: usize,
    positions: usize,
    committed: bool,
}

impl<'a> KvAppend<'a> {
    pub(super) fn new(
        cache: &'a mut LayerKvCache,
        keys: &[f32],
        values: &[f32],
    ) -> io::Result<Self> {
        let next = cache.seq_len.checked_add(1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Speech KV position overflow.")
        })?;
        cache.k.try_reserve(keys.len()).map_err(io::Error::other)?;
        cache
            .v
            .try_reserve(values.len())
            .map_err(io::Error::other)?;
        let append = Self {
            keys: cache.k.len(),
            values: cache.v.len(),
            positions: cache.seq_len,
            cache,
            committed: false,
        };
        append.cache.k.extend_from_slice(keys);
        append.cache.v.extend_from_slice(values);
        append.cache.seq_len = next;
        Ok(append)
    }

    pub(super) const fn cache(&self) -> &LayerKvCache {
        self.cache
    }

    pub(super) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for KvAppend<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.cache.k.truncate(self.keys);
            self.cache.v.truncate(self.values);
            self.cache.seq_len = self.positions;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io, panic::AssertUnwindSafe};

    use super::{KvAppend, LayerKvCache};

    #[test]
    fn decoder_kv_append_rolls_back_on_unwind_and_commits_only_explicitly() -> io::Result<()> {
        let mut cache = LayerKvCache {
            k: vec![-0.0, 0.25],
            v: vec![0.5, -0.75],
            seq_len: 1,
        };
        let interrupted = std::panic::catch_unwind(AssertUnwindSafe(|| -> io::Result<()> {
            let _append = KvAppend::new(&mut cache, &[1.0, 2.0], &[3.0, 4.0])?;
            // Resume an injected fault without invoking the process panic hook.
            std::panic::resume_unwind(Box::new("injected decoder fault"));
        }));
        assert!(interrupted.is_err());
        assert_eq!(
            cache.k.iter().map(|n| n.to_bits()).collect::<Vec<_>>(),
            [0x8000_0000, 0x3e80_0000]
        );
        assert_eq!(cache.v, [0.5, -0.75]);
        assert_eq!(cache.seq_len, 1);
        KvAppend::new(&mut cache, &[1.0, 2.0], &[3.0, 4.0])?.commit();
        assert_eq!(cache.k, [-0.0, 0.25, 1.0, 2.0]);
        assert_eq!(cache.v, [0.5, -0.75, 3.0, 4.0]);
        assert_eq!(cache.seq_len, 2);
        cache.seq_len = usize::MAX;
        assert!(KvAppend::new(&mut cache, &[5.0], &[6.0]).is_err());
        assert_eq!(cache.k.len(), 4);
        assert_eq!(cache.v.len(), 4);
        assert_eq!(cache.seq_len, usize::MAX);
        Ok(())
    }
}
