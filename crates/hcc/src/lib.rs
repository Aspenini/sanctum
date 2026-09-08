use holyc_codegen::{compile_jit, CodegenError, JitProgram};
use holyc_parse::Parser;
use holyc_sema::Sema;
use holyc_syntax::{lex_buffer, preprocess, PreprocessOpts, Session, SyntaxError};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HccError {
    #[error(transparent)]
    Syntax(#[from] SyntaxError),
    #[error(transparent)]
    Codegen(#[from] CodegenError),
    #[error("{0}")]
    Msg(String),
}

pub struct CompileOptions {
    pub include_dirs: Vec<std::path::PathBuf>,
    pub system_root: Option<std::path::PathBuf>,
    pub cores: u32,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            include_dirs: Vec::new(),
            system_root: None,
            cores: 1,
        }
    }
}

pub fn compile_source(path: &str, src: &str) -> Result<JitProgram, HccError> {
    let mut session = Session::new();
    let tokens = lex_buffer(&mut session, path, src)?;
    let mut parser = Parser::new(&tokens, path, src);
    let mut ast = parser.parse_module()?;
    let mut sema = Sema::new(path, src);
    sema.run(&mut ast)?;
    let syms = tos_runtime::jit_symbols();
    let refs: Vec<(&str, *const u8)> = syms.iter().map(|(n, p)| (*n, *p)).collect();
    Ok(compile_jit(&ast, &sema, &refs)?)
}

pub fn compile_file(path: &Path, opts: &CompileOptions) -> Result<JitProgram, HccError> {
    let mut session = Session::new();
    let pp = PreprocessOpts {
        include_dirs: opts.include_dirs.clone(),
        system_root: opts.system_root.clone(),
    };
    let tokens = preprocess(&mut session, path, &pp)?;
    let src = session
        .file(tokens.first().map(|t| t.span.file).unwrap_or(0))
        .map(|f| f.src.clone())
        .unwrap_or_default();
    let path_str = path.display().to_string();
    let mut parser = Parser::new(&tokens, &path_str, &src);
    let mut ast = parser.parse_module()?;
    let mut sema = Sema::new(&path_str, &src);
    sema.run(&mut ast)?;
    let syms = tos_runtime::jit_symbols();
    let refs: Vec<(&str, *const u8)> = syms.iter().map(|(n, p)| (*n, *p)).collect();
    Ok(compile_jit(&ast, &sema, &refs)?)
}

pub fn run_source(path: &str, src: &str) -> Result<(), HccError> {
    let mut prog = compile_source(path, src)?;
    prog.run()?;
    Ok(())
}

pub fn run_file(path: &Path, opts: &CompileOptions) -> Result<(), HccError> {
    let mut prog = compile_file(path, opts)?;
    prog.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_world() {
        tos_runtime::capture_begin();
        run_source(
            "hello.HC",
            "U0 Main()\n{\n  \"Hello world\\n\";\n}\nMain;\n",
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"Hello world\n");
    }

    #[test]
    fn add_and_print() {
        tos_runtime::capture_begin();
        run_source(
            "add.HC",
            "U0 Main()\n{\n  I64 x=2;\n  I64 y=3;\n  \"%d\\n\",x+y;\n}\nMain;\n",
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"5\n");
    }

    #[test]
    fn if_while_for() {
        tos_runtime::capture_begin();
        run_source(
            "ctrl.HC",
            r#"
U0 Main()
{
  I64 i=0;
  I64 s=0;
  if (1)
    s=s+1;
  while (i<3) {
    s=s+i;
    i++;
  }
  for (I64 j=0; j<2; j++)
    s=s+10;
  "%d\n",s;
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        // 1 + (0+1+2) + 10+10 = 24
        assert_eq!(out, b"24\n");
    }

    #[test]
    fn function_args_and_return() {
        tos_runtime::capture_begin();
        run_source(
            "fun.HC",
            r#"
I64 Add(I64 a, I64 b)
{
  return a+b;
}
U0 Main()
{
  "%d\n",Add(20,22);
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"42\n");
    }

    #[test]
    fn chained_compare_and_define() {
        tos_runtime::capture_begin();
        run_source(
            "cmp.HC",
            r#"
#define LO 5
U0 Main()
{
  I64 age=13;
  if (LO<=age<20)
    "teen\n";
  else
    "no\n";
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"teen\n");
    }

    #[test]
    fn include_and_file() {
        tos_runtime::capture_begin();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/holyc/inc_main.HC");
        run_file(&path, &CompileOptions::default())
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"included\n");
    }
}
