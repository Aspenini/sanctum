# Sanctum

Sanctum is a Rust workspace for running TempleOS HolyC programs directly on
Windows, Linux, and macOS. It has three related layers:

- **`holycc`** is the HolyC compiler command-line tool (backed by the `holyc`
  Rust crate). It parses, checks,
  and JIT-compiles HolyC to native code with Cranelift. The same compiler API is
  used by Sanctum; future AOT output belongs here as well.
- **`templeos-compat`** is the reusable TempleOS compatibility layer. It
  provides host implementations of the TempleOS ABI, graphics, audio, input,
  tasks, registry calls, DolDoc resources, and other APIs used by compiled
  programs. It is not a hardware emulator and does not boot TempleOS.
- **`sanctum`** is the graphical library and runner. Point it at a containing
  folder and it imports top-level HolyC files as standalone programs plus each
  child folder as a multi-file project. It compiles with the `holyc` compiler
  library and runs each program in an isolated child process through
  `templeos-compat`.

The project does not contain, download, or redistribute TempleOS. Self-contained
HolyC programs run without an ISO. Programs that use `::/` includes or other OS
resources need an extracted TempleOS directory selected from Settings. Sanctum
does not accept, copy, download, or extract ISO images.

## Run Sanctum

```text
cargo run -p sanctum
```

The Slint desktop app provides a searchable library with favorites and recents,
editable entrypoints and cover art, a live indexed-color canvas, TempleOS menu
actions, keyboard input, sound controls, logs, fullscreen mode, and graceful
Stop/Restart controls. The live game can be detached into its own window and
re-docked without restarting its isolated runner. A populated rendered frame
can automatically become a game's 4:3 library cover. A multi-file project's
entrypoint is selected on its first run and remembered. The sidebar Run action
can also launch a chosen `.HC` file once without adding it to the library.

Settings, library metadata, covers, and registry saves normally live in the
platform user-data directory. Portable mode can move Sanctum-owned data into
`SanctumData` beside the executable when that location is writable. Imported
HolyC projects and the selected TempleOS directory always remain in place and
are never copied or modified.

## Run the compiler

```text
cargo run -p holyc --bin holycc -- --run tests/holyc/hello.HC
```

Without `--run`, `holycc` performs compile-only validation. The CLI remains
independent of the Sanctum GUI.

The implemented language/runtime subset includes preprocessing, TempleOS
operator precedence, packed classes and unions, globals, pointers, callbacks,
control flow, integer and `F64` arithmetic, tasks, menus and settings, heap and
queue primitives, indexed graphics and 3D mesh sprites, registry persistence,
CP437 source/output handling, and CPAL square-wave audio. Missing or lost audio
devices produce a warning and silent fallback instead of preventing launch.

`TempleOS/Demo/Games/Talons.HC` compiles end-to-end and has JIT acceptance tests
covering terrain initialization, its real draw callback, a non-empty 640x480
frame, input, cleanup, and the actual fishing loop. The gameplay test locates a
generated fish through Talons' own object queues, verifies that approaching it
lowers and visibly renders the claws, and verifies that a catch removes it and
decrements the fish counter. A smaller tracked graphical smoke program also
exercises the complete Sanctum runner lifecycle, including frame delivery,
input, clean exit, and repeated launches.

This is intentionally a growing compatibility subset, not complete TempleOS
compatibility. Keep a local `TempleOS/` tree as a language/API reference if
needed; that directory is ignored and is not shipped with the repository.
