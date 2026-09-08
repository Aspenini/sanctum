use hcc::{CompileOptions, compile_file, run_file, run_source};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!(
            "hcc -- HolyC AOT compiler\n\n  hcc [--run] [--cores N] [-I dir] <file.HC>\n  hcc --eval '<source>'\n"
        );
        return ExitCode::SUCCESS;
    }

    let mut opts = CompileOptions::default();
    let mut eval: Option<String> = None;
    let mut file: Option<PathBuf> = None;
    let mut should_run = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--run" | "-r" => should_run = true,
            "--eval" => {
                i += 1;
                eval = args.get(i).cloned();
            }
            "--cores" => {
                i += 1;
                opts.cores = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1);
            }
            "-I" => {
                i += 1;
                if let Some(d) = args.get(i) {
                    opts.include_dirs.push(PathBuf::from(d));
                }
            }
            s if s.starts_with('-') => {
                eprintln!("unknown option {s}");
                return ExitCode::from(2);
            }
            s => file = Some(PathBuf::from(s)),
        }
        i += 1;
    }

    // Default ::/ to ./TempleOS if present.
    if opts.system_root.is_none() {
        let cand = PathBuf::from("TempleOS");
        if cand.is_dir() {
            opts.system_root = Some(cand);
        }
    }

    let res = if let Some(src) = eval {
        run_source("<eval>", &src)
    } else if let Some(path) = file {
        if should_run {
            run_file(&path, &opts)
        } else {
            compile_file(&path, &opts).map(|_| ())
        }
    } else {
        eprintln!("no input file");
        return ExitCode::from(2);
    };

    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
