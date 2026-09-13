//! Reusable TempleOS compatibility services for JIT- and AOT-compiled HolyC.
//!
//! The `tos-*` crates are implementation units. This crate is their public
//! integration boundary: compiler frontends and graphical hosts should depend
//! on `templeos-compat`, not assemble individual subsystems themselves.

pub use tos_abi as abi;
pub use tos_doldoc as doldoc;
pub use tos_gr as graphics;
pub use tos_host as host;
pub use tos_host::GlobalBinding;
pub use tos_host::HostMode;
pub use tos_runtime as runtime;

/// Native functions that a HolyC JIT must make available by their ABI names.
///
/// The same `#[no_mangle]` functions are retained in this crate's `staticlib`
/// output, allowing an AOT backend to link against the compatibility layer.
pub fn jit_symbols() -> Vec<(&'static str, *const u8)> {
    let mut symbols = runtime::jit_symbols();
    symbols.extend(graphics::jit_symbols());
    symbols.extend(host::jit_symbols());
    symbols
}

/// Prepare the compatibility runtime for one compiled program invocation.
pub fn prepare(interactive: bool) {
    prepare_with_mode(if interactive {
        HostMode::NativeWindow
    } else {
        HostMode::Headless
    });
}

pub fn prepare_with_mode(mode: HostMode) {
    host::set_host_mode(mode);
    runtime::set_background_tasks_enabled(mode != HostMode::Headless);
    host::reset();
}

/// Bind finalized compiled globals for TempleOS services such as `RegExe`.
pub fn bind_globals(bindings: &[GlobalBinding<'_>]) {
    host::bind_globals(bindings);
}

/// Quiesce background HolyC tasks before releasing compiled code.
pub fn shutdown() {
    runtime::cancel_background_tasks();
    host::shutdown();
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_the_combined_jit_abi() {
        let symbols = super::jit_symbols();
        let unique = symbols
            .iter()
            .map(|(name, _)| *name)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            unique.len(),
            symbols.len(),
            "duplicate compatibility symbol"
        );
        for required in [
            "tos_Print",
            "tos_DCTransform",
            "tos_GrLine3",
            "tos_Refresh",
            "tos_Play",
        ] {
            assert!(symbols.iter().any(|(name, _)| *name == required));
        }
    }
}
