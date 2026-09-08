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
core runtime primitives (`Print`, heaps, queues, bits, time/random, `Fs`/`Gs`).

**Next:** matrix/math runtime APIs, 16-color `CDC`/`Gr*` in `tos-gr`, DolDoc
sprite bins, then end-to-end `TempleOS/Demo/Games/Talons.HC` bring-up.

TempleOS HolyC stays source-compatible (no `F32`/`auto`). Keep a local
`TempleOS/` tree as the language/API spec if you have one; it is gitignored and
not shipped in this repo.
