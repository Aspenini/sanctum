# Sanctum

Sanctum is a workspace for three related layers, written in idiomatic Rust
(edition 2024):

- **`holyc`** is the compiler and command-line tool. It owns HolyC parsing,
  type checking, and code generation. It JIT-runs programs today; AOT output
  belongs here as that backend is completed.
- **`templeos-compat`** is the reusable compatibility layer. It provides Rust
  implementations of the TempleOS ABI, kernel calls, graphics, DolDoc resource
  handling, host input, and other libraries expected by compiled programs. It
  can supply symbols to the JIT or be linked as a static library for AOT output.
- **Sanctum** is the planned graphical, emulator-style launcher. Users will
  point it at a HolyC program or game directory and Sanctum will invoke `holyc`
  and run the result through `templeos-compat`. It is not a hardware emulator
  and does not boot TempleOS.

TempleOS HolyC programs already compile here and run directly on the host.
HolyC written for Sanctum stays within TempleOS language rules and public APIs
so the original TempleOS compiler can compile it too. The `TempleOS/` tree is
the specification and test corpus, not the OS being booted.

```
cargo run -p holyc -- --run tests/holyc/hello.HC
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
`Fs`/`Gs`). The compatibility graphics subsystem provides indexed-color device
contexts, depth buffers, line/polygon rasterization, blits, bitmap and 3D mesh
sprites, interpolation, symmetry, TempleOS mesh lighting, probability
dithering, and HUD text.

`TempleOS/Demo/Games/Talons.HC` now compiles end-to-end and passes a JIT smoke
run through terrain initialization, its real `DrawIt` callback, a non-empty
640×480 rendered frame with its embedded aircraft and terrain art, and cleanup.
The DolDoc loader also repairs `0x05` bytes stripped by TempleOS's text-export
mode, so the checked-in ASCII Talons source retains structurally valid meshes.
`--run` opens the framebuffer in a native window, enables background HolyC
tasks, and feeds Escape, Enter, Space, and arrow keys through `ScanKey`; without
`--run`, `holyc` performs compile-only validation.

**Next:** add audio synthesis and broaden TempleOS API coverage beyond the
subset exercised by Talons. The game is at the basic playable milestone; sound
and closer behavioral/visual parity remain.

TempleOS HolyC stays source-compatible (no `F32`/`auto`). Keep a local
`TempleOS/` tree as the language/API spec if you have one; it is gitignored and
not shipped in this repo.
