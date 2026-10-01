# Sanctum

Sanctum is a Rust workspace for running TempleOS HolyC programs directly on
Windows, Linux, macOS, and Android. It has three related layers:

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

The Slint desktop app provides a searchable and sortable library, editable
entrypoints and cover art, a live indexed-color canvas, TempleOS menu actions,
keyboard input, sound controls, logs, fullscreen mode, and graceful Stop/Restart
controls. The live game can be detached into its own window and re-docked
without restarting its isolated runner. The first sufficiently populated frame
received after one second of execution can automatically become a game's 4:3
library cover. A multi-file project's
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

## Build for Android

The Android shell lives in `android-project` and packages the same Slint UI,
compiler, and compatibility runtime into an ARM64 APK. Install Android SDK 35,
NDK `30.0.16248370`, the Rust `aarch64-linux-android` target, and `cargo-apk`,
then run:

```text
cd android-project
./gradlew assembleDebug
```

Set `ANDROID_HOME` to the installed Android SDK before invoking Gradle.
`ANDROID_NDK_ROOT`/`ANDROID_NDK_HOME` may override the NDK selected from that
SDK; otherwise the build uses the pinned NDK version above. The APK is written
under `android-project/app/build/outputs/apk/debug`. Android cannot spawn a child
executable, so its runner uses the same authenticated IPC protocol over an
in-process worker thread; desktop runners remain isolated child processes.

The implemented language/runtime subset includes preprocessing, TempleOS
operator precedence, packed classes and unions, globals, pointers, callbacks,
control flow including `goto`, `try`/`catch`, and nested functions, persistent local
`static` storage, user-defined variadic functions with `argc`/`argv`, integer and `F64` arithmetic
(including the HolyC power operator), tasks, menus and settings, heap and queue
primitives, indexed graphics with 3D mesh sprites and circles, mouse and
keyboard input, registry
persistence, CP437 source/output handling, and CPAL square-wave audio. Missing
or lost audio devices produce a warning and silent fallback instead of
preventing launch.

Unsigned integer division, remainder, shifts, and comparisons preserve HolyC's
operand types, including compound assignments and chained comparisons. Integer
conversion to `F64` remains signed, following TempleOS. User-defined default
arguments work for omitted arguments and explicit argument holes. Variadic
arguments use raw 64-bit slots, preserving floating-point bit patterns.
Local statics initialize once before module-level code runs and retain their
storage across function calls; initializers do not execute during compile-only
validation. Compile-time `#exe` execution and inline assembly remain unsupported.

Read-only TempleOS file calls (`FileRead`, `FileFind`, `FilesFind`, `Cd`,
`IsDir`, `DirCur`) resolve inside the program directory and the selected
TempleOS tree (`::/`). Writes are not implemented; paths cannot climb out of
those roots.

A checked-in compile matrix in `crates/holyc/tests/demo_games_matrix.rs`
tracks every `TempleOS/Demo/Games` HolyC file: Talons, TicTacToe, and Castle Frankenstein compile
today, and each remaining demo pins its current first error so progress or
regressions show up in local test runs. Run the complete suite with:

```text
cargo test --workspace --locked
```

The reference-game tests require the local `TempleOS/` tree; they skip when it
is absent. Self-contained language and runner tests do not require that tree.

`TempleOS/Demo/Games/Talons.HC` compiles end-to-end and has JIT acceptance tests
covering terrain initialization, its real draw callback, a non-empty 640x480
frame, input, cleanup, and the actual fishing loop. The gameplay test locates a
generated fish through Talons' own object queues, verifies that approaching it
lowers and visibly renders the claws, and verifies that a catch removes it and
decrements the fish counter. `TempleOS/Demo/Games/TicTacToe.HC` is covered the
same way: a helper drives `ms` clicks through a winning X column and throws to
leave the outer `try` loop. A smaller tracked graphical smoke program also
exercises the complete Sanctum runner lifecycle, including frame delivery,
input, clean exit, and repeated launches.

This is intentionally a growing compatibility subset, not complete TempleOS
compatibility. Keep a local `TempleOS/` tree as a language/API reference if
needed; that directory is ignored and is not shipped with the repository.
