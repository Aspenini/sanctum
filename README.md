# Sanctum

A HolyC ahead-of-time compiler and TempleOS-compatible runtime, written in
idiomatic Rust (edition 2024).

TempleOS HolyC programs compile here and run on the host. HolyC written for
Sanctum stays within TempleOS language rules and public APIs so the original
TempleOS compiler can compile it too. Kernel, DolDoc, and graphics APIs that
HolyC expects are reimplemented in Rust — the `TempleOS/` tree is the spec and
test corpus, not the OS we boot.

```
cargo run -p hcc -- --run tests/holyc/hello.HC
```

Prints `Hello world`. HolyC is compiled to native code with Cranelift and linked
against a Rust runtime (`Print`, heaps, …).

**Now:** CP437/binary-tail source loading, lexer (DolDoc `$` skip / `$IB`),
preprocessor (`#include` `#define` `#if`), parser (functions, control flow,
initializers, multi-declarations, chained compares, TempleOS precedence), packed
`class`/`union` layout, globals and field/index access, Cranelift JIT `--run`, and
mixed integer/`F64` arithmetic, indirect callbacks, switch/loop control flow,
fixed-point matrix/vector math, task records and deterministic single-core
`Spawn`, and core runtime primitives (`Print`, heaps, queues, bits, time/random,
`Fs`/`Gs`). `tos-gr` now provides indexed-color device contexts, depth buffers,
line/polygon rasterization, blits, and the sprite-facing ABI.

`TempleOS/Demo/Games/Talons.HC` now compiles end-to-end and passes a JIT smoke
run through terrain initialization, its real `DrawIt` callback, a non-empty
640×480 rendered frame, and cleanup. `--run` opens that framebuffer in a native
window, enables background HolyC tasks, and feeds Escape, Enter, and arrow keys
through `ScanKey`; without `--run`, `hcc` performs compile-only validation.

**Next:** decode DolDoc sprite-bin payloads, finish sprite rasterization, and add
audio synthesis. Talons currently has an interactive graphics preview, but its
missing embedded sprites keep it short of the intended game presentation.

TempleOS HolyC stays source-compatible (no `F32`/`auto`). Keep a local
`TempleOS/` tree as the language/API spec if you have one; it is gitignored and
not shipped in this repo.
