//! Explicit ASR compiler ISA selection without changing the portable ARM64 floor.

/// Whether the ASR executable stays on this host or is prepared for distribution.
#[derive(Clone, Copy, Debug)]
pub(crate) enum AsrBuildTarget {
    /// Host-bound source outputs may use independently observed OS features.
    Host,
    /// Distributed candidates keep the ARMv8-A baseline regardless of build host.
    Portable,
}

impl AsrBuildTarget {
    /// Select only optional DOTPROD and I8MM instructions observed by the OS.
    /// Missing observations preserve the baseline; neither feature implies the other.
    pub(crate) const fn architecture(self, observed: Option<(bool, bool)>) -> &'static str {
        match (self, observed) {
            (Self::Host, Some((true, true))) => "armv8-a+dotprod+i8mm",
            (Self::Host, Some((true, false))) => "armv8-a+dotprod",
            (Self::Host, Some((false, true))) => "armv8-a+i8mm",
            _ => "armv8-a",
        }
    }
}

/// ASR-only `CMake` options; clang-cl receives its ISA through the explicit escape.
pub(crate) fn cmake_args(architecture: &str) -> String {
    format!(
        "-DGGML_NATIVE=OFF -DCMAKE_C_FLAGS=/clang:-march={architecture} -DCMAKE_CXX_FLAGS=/clang:-march={architecture} -DGGML_OPENMP=OFF -DTRANSCRIBE_USE_OPENMP=OFF -DTRANSCRIBE_USE_SYSTEM_BLAS=OFF -DCMAKE_C_COMPILER_TARGET=aarch64-pc-windows-msvc -DCMAKE_CXX_COMPILER_TARGET=aarch64-pc-windows-msvc"
    )
}

#[cfg(test)]
mod tests {
    use super::{AsrBuildTarget, cmake_args};

    #[test]
    fn asr_cpu_host_features_are_independent_and_unknown_preserves_baseline() {
        for (observed, expected) in [
            (None, "armv8-a"),
            (Some((false, false)), "armv8-a"),
            (Some((true, false)), "armv8-a+dotprod"),
            (Some((false, true)), "armv8-a+i8mm"),
            (Some((true, true)), "armv8-a+dotprod+i8mm"),
        ] {
            assert_eq!(AsrBuildTarget::Host.architecture(observed), expected);
        }
    }

    #[test]
    fn asr_cpu_portable_target_never_inherits_build_host_extensions() {
        for observed in [
            None,
            Some((false, false)),
            Some((true, false)),
            Some((false, true)),
            Some((true, true)),
        ] {
            assert_eq!(AsrBuildTarget::Portable.architecture(observed), "armv8-a");
        }
    }

    #[test]
    fn asr_cpu_cmake_passes_selected_isa_to_both_clang_cl_languages() {
        for target in [AsrBuildTarget::Host, AsrBuildTarget::Portable] {
            let architecture = target.architecture(Some((true, true)));
            let args = cmake_args(architecture);
            let args: Vec<_> = args.split_ascii_whitespace().collect();
            for expected in [
                format!("-DCMAKE_C_FLAGS=/clang:-march={architecture}"),
                format!("-DCMAKE_CXX_FLAGS=/clang:-march={architecture}"),
                "-DGGML_NATIVE=OFF".to_owned(),
                "-DGGML_OPENMP=OFF".to_owned(),
                "-DTRANSCRIBE_USE_OPENMP=OFF".to_owned(),
                "-DTRANSCRIBE_USE_SYSTEM_BLAS=OFF".to_owned(),
            ] {
                assert!(args.contains(&expected.as_str()));
            }
        }
    }
}
