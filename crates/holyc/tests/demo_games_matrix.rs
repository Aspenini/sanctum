//! Compile matrix for `TempleOS/Demo/Games`.
//!
//! Each HolyC file is compile-only. `Compiles` must keep compiling.
//! `Blocked` entries pin the current first error so a new failure, or an
//! unexpected success, is visible.

use holyc::{CompileOptions, compile_file};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Expect {
    Compiles,
    Blocked(&'static str),
}

/// Paths are relative to `TempleOS/Demo/Games`.
const MATRIX: &[(&str, Expect)] = &[
    (
        "BattleLines.HC",
        Expect::Blocked("operator Mod is invalid for F64"),
    ),
    (
        "BigGuns.HC",
        Expect::Blocked("cannot type field base for `pos`"),
    ),
    (
        "BlackDiamond.HC",
        Expect::Blocked("operator Mod is invalid for F64"),
    ),
    (
        "BomberGolf.HC",
        Expect::Blocked("unknown function `GrRect3`"),
    ),
    (
        "CastleFrankenstein.HC",
        Expect::Blocked("unknown function `Line`"),
    ),
    (
        "CharDemo.HC",
        Expect::Blocked("cannot type field base for `hide_row`"),
    ),
    ("CircleTrace.HC", Expect::Blocked("unknown function `Sqr`")),
    (
        "Collision.HC",
        Expect::Blocked("unknown function `GrCircle`"),
    ),
    (
        "Digits.HC",
        Expect::Blocked("no member `text_attr` on CTask"),
    ),
    ("DunGen.HC", Expect::Blocked("unknown function `Line`")),
    (
        "ElephantWalk.HC",
        Expect::Blocked("cannot type field base for `flags`"),
    ),
    ("FlapBat.HC", Expect::Blocked("unknown function `MaxI64`")),
    ("FlatTops.HC", Expect::Blocked("expected )")),
    ("Halogen.HC", Expect::Blocked("unknown function `GrPlot`")),
    (
        "MassSpring.HC",
        Expect::Blocked("cannot type field base for `next_spring`"),
    ),
    ("Maze.HC", Expect::Blocked("expected (")),
    ("RainDrops.HC", Expect::Blocked("unknown function `GrLine`")),
    (
        "RawHide.HC",
        Expect::Blocked("unknown identifier `MAP_HEIGHT`"),
    ),
    ("Rocket.HC", Expect::Blocked("unknown function `Arg`")),
    (
        "RocketScience.HC",
        Expect::Blocked("unknown function `Arg`"),
    ),
    (
        "Squirt.HC",
        Expect::Blocked("cannot type field base for `drag_v2`"),
    ),
    (
        "Stadium/Stadium.HC",
        Expect::Blocked("unknown function `Sprite3ZB`"),
    ),
    (
        "Stadium/StadiumGen.HC",
        Expect::Blocked("expected identifier"),
    ),
    ("Talons.HC", Expect::Compiles),
    ("TheDead.HC", Expect::Blocked("unknown function `GrLine`")),
    ("TicTacToe.HC", Expect::Compiles),
    ("TreeCheckers.HC", Expect::Blocked("expected (")),
    ("Varoom.HC", Expect::Blocked("unknown function `R2P`")),
    (
        "Wenceslas.HC",
        Expect::Blocked("unknown function `GrPlot3`"),
    ),
    ("Whap.HC", Expect::Blocked("unknown function `GrRect`")),
    ("Zing.HC", Expect::Blocked("unknown function `GrBorder`")),
    (
        "ZoneOut.HC",
        Expect::Blocked("unknown function `Mat4x4RotY`"),
    ),
];

fn games_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../TempleOS/Demo/Games")
}

fn rel_slash(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn discover_games(root: &Path) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("HC"))
            {
                files.insert(rel_slash(&path, root));
            }
        }
    }
    files
}

fn compile_game(root: &Path, rel: &str) -> Result<(), String> {
    let path = root.join(rel);
    let opts = CompileOptions {
        system_root: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../TempleOS")),
        ..CompileOptions::default()
    };
    compile_file(&path, &opts)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[test]
fn demo_games_compile_matrix_matches_expected() {
    let root = games_root();
    if !root.is_dir() {
        eprintln!(
            "skipping Demo/Games compile matrix: {} is absent",
            root.display()
        );
        return;
    }

    let discovered = discover_games(&root);
    let expected = MATRIX
        .iter()
        .map(|(path, expect)| (*path, *expect))
        .collect::<BTreeMap<_, _>>();

    let mut failures = Vec::new();
    for extra in &discovered {
        if !expected.contains_key(extra.as_str()) {
            failures.push(format!(
                "untracked Demo/Games file `{extra}` — add it to MATRIX"
            ));
        }
    }
    for missing in expected.keys() {
        if !discovered.contains(*missing) {
            failures.push(format!(
                "MATRIX lists `{missing}` but the file is not in Demo/Games"
            ));
        }
    }

    let mut compiles = 0usize;
    let mut blocked = 0usize;
    for (rel, expect) in &expected {
        if !discovered.contains(*rel) {
            continue;
        }
        match (*expect, compile_game(&root, rel)) {
            (Expect::Compiles, Ok(())) => compiles += 1,
            (Expect::Compiles, Err(error)) => failures.push(format!(
                "`{rel}` should compile, but failed: {error}"
            )),
            (Expect::Blocked(needle), Ok(())) => failures.push(format!(
                "`{rel}` compiled, but MATRIX still lists it as blocked on `{needle}`"
            )),
            (Expect::Blocked(needle), Err(error)) if error.contains(needle) => blocked += 1,
            (Expect::Blocked(needle), Err(error)) => failures.push(format!(
                "`{rel}` is still blocked, but the first error moved:\n  expected to contain: {needle}\n  actual: {error}"
            )),
        }
    }

    eprintln!(
        "Demo/Games compile matrix: {compiles} compile, {blocked} blocked, {} tracked",
        MATRIX.len()
    );
    if !failures.is_empty() {
        panic!(
            "Demo/Games compile matrix drifted:\n{}",
            failures
                .iter()
                .map(|failure| format!("- {failure}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
