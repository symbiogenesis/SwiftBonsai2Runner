//! Request-owned acoustic decoder capacity; no logical frame state is reused.
#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "Talker shares this private engine workspace without exposing it to library callers."
)]

use std::io;

use super::LayerKvCache;

#[derive(Default)]
pub(crate) struct PredictorWorkspace {
    layers: Vec<LayerKvCache>,
    seen: Vec<u8>,
}

impl PredictorWorkspace {
    fn reset(&mut self) {
        for layer in &mut self.layers {
            layer.k.clear();
            layer.v.clear();
            layer.seq_len = 0;
        }
        self.seen.clear();
    }

    pub(super) fn frame(&mut self, layers: usize) -> io::Result<Frame<'_>> {
        self.reset();
        if self.layers.len() != layers {
            self.layers.clear();
            self.layers.try_reserve(layers).map_err(io::Error::other)?;
            self.layers.resize_with(layers, LayerKvCache::default);
        }
        Ok(Frame(self))
    }
}

pub(super) struct Frame<'a>(&'a mut PredictorWorkspace);

impl Frame<'_> {
    pub(super) fn buffers(&mut self) -> (&mut [LayerKvCache], &mut Vec<u8>) {
        (&mut self.0.layers, &mut self.0.seen)
    }
}

impl Drop for Frame<'_> {
    fn drop(&mut self) {
        self.0.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::PredictorWorkspace;
    use std::{io, panic::AssertUnwindSafe};

    #[test]
    fn predictor_workspace_clears_every_frame_on_return_and_unwind_without_reallocating()
    -> io::Result<()> {
        let mut workspace = PredictorWorkspace::default();
        let allocations = {
            let mut frame = workspace.frame(2)?;
            let (layers, seen) = frame.buffers();
            for (index, layer) in layers.iter_mut().enumerate() {
                layer.k.extend_from_slice(&[-0.0, 0.25, 0.5, 0.75]);
                layer.v.extend_from_slice(&[1.0, 1.25, 1.5, 1.75]);
                layer.seq_len = index + 1;
            }
            seen.extend_from_slice(&[1; 8]);
            layers
                .iter()
                .map(|layer| {
                    (
                        layer.k.as_ptr(),
                        layer.v.as_ptr(),
                        layer.k.capacity(),
                        layer.v.capacity(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert!(
            workspace
                .layers
                .iter()
                .all(|layer| layer.k.is_empty() && layer.v.is_empty() && layer.seq_len == 0)
        );
        assert_eq!(workspace.seen, Vec::<u8>::new());
        let interrupted = std::panic::catch_unwind(AssertUnwindSafe(|| -> io::Result<()> {
            let mut frame = workspace.frame(2)?;
            let (layers, seen) = frame.buffers();
            for (layer, allocation) in layers.iter_mut().zip(&allocations) {
                assert_eq!(
                    (
                        layer.k.as_ptr(),
                        layer.v.as_ptr(),
                        layer.k.capacity(),
                        layer.v.capacity()
                    ),
                    *allocation
                );
                layer.k.extend_from_slice(&[2.0; 4]);
                layer.v.extend_from_slice(&[3.0; 4]);
                layer.seq_len = 2;
            }
            seen.extend_from_slice(&[1; 8]);
            std::panic::resume_unwind(Box::new("injected predictor fault"));
        }));
        assert!(interrupted.is_err());
        assert!(
            workspace
                .layers
                .iter()
                .all(|layer| layer.k.is_empty() && layer.v.is_empty() && layer.seq_len == 0)
        );
        assert_eq!(workspace.seen, Vec::<u8>::new());
        drop(workspace.frame(1)?);
        assert_eq!(workspace.layers.len(), 1);
        assert!(workspace.layers[0].k.is_empty() && workspace.layers[0].v.is_empty());
        Ok(())
    }
}
