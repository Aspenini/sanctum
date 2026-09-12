# Sanctum

Sanctum is a Rust workspace for running TempleOS HolyC programs directly on
Windows, Linux, and macOS. It has three related layers:

- **`holyc`** is the HolyC compiler and command-line tool. It parses, checks,
  and JIT-compiles HolyC to native code with Cranelift. The same compiler API is
  used by Sanctum; future AOT output belongs here as well.
- **`templeos-compat`** is the reusable TempleOS compatibility layer. It
  provides host implementations of the TempleOS ABI, graphics, audio, input,
  tasks, registry calls, DolDoc resources, and other APIs used by compiled
  programs. It is not a hardware emulator and does not boot TempleOS.
- **`sanctum`** is the graphical library and runner. It imports HolyC files or
  project folders, compiles them with `holyc`, and runs each program in an
  isolated child process through `templeos-compat`.

The project does not contain, download, or redistribute TempleOS. Self-contained
HolyC programs run without an ISO. Programs that use `::/` includes or other OS
resources need a locally supplied TempleOS ISO, which Sanctum can safely extract
to application storage from Settings.

## Run Sanctum

```text
cargo run -p sanctum
```

The desktop app provides a searchable library with favorites and recents,
editable entrypoints and cover art, a live indexed-color canvas, TempleOS menu
actions, keyboard input, sound controls, logs, fullscreen mode, and graceful
Stop/Restart controls. A populated rendered frame can automatically become a
game's 4:3 library cover.

Settings, library metadata, covers, registry saves, and the extracted TempleOS
tree normally live in the platform user-data directory. Portable mode can move
Sanctum-owned data into `SanctumData` beside the executable when that location
is writable. Imported HolyC projects always remain in place and are never
copied or modified.

## Run the compiler

```text
cargo run -p holyc -- --run tests/holyc/hello.HC
```

Without `--run`, `holyc` performs compile-only validation. The CLI remains
independent of the Sanctum GUI.

The implemented language/runtime subset includes preprocessing, TempleOS
operator precedence, packed classes and unions, globals, pointers, callbacks,
control flow, integer and `F64` arithmetic, tasks, menus and settings, heap and
queue primitives, indexed graphics and 3D mesh sprites, registry persistence,
CP437 source/output handling, and CPAL square-wave audio. Missing or lost audio
devices produce a warning and silent fallback instead of preventing launch.

`TempleOS/Demo/Games/Talons.HC` compiles end-to-end and has a JIT smoke test
covering terrain initialization, its real draw callback, a non-empty 640x480
frame, and cleanup. A smaller tracked graphical smoke program also exercises
the complete Sanctum runner lifecycle, including frame delivery, input, clean
exit, and repeated launches.

This is intentionally a growing compatibility subset, not complete TempleOS
compatibility. Keep a local `TempleOS/` tree as a language/API reference if
needed; that directory is ignored and is not shipped with the repository.
