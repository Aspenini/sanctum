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
    fn adjacent_strings_are_concatenated() {
        tos_runtime::capture_begin();
        run_source(
            "strings.HC",
            "U0 Main() { Print(\"Hello \" \"world\\n\"); } Main;",
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
  for (i=0,j=2; i<2; i++,j--)
    s=s+j;
  "%d\n",s;
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        // 1 + (0+1+2) + 10+10 + (2+1) = 27
        assert_eq!(out, b"27\n");
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

    #[test]
    fn class_fields_packed() {
        tos_runtime::capture_begin();
        run_source(
            "pt.HC",
            r#"
class Point
{
  I64 x,y;
};
U0 Main()
{
  Point p;
  p.x=3;
  p.y=4;
  "%d %d %d\n",p.x,p.y,sizeof(Point);
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"3 4 16\n");
    }

    #[test]
    fn nested_packed_fields_arrays_and_offsets() {
        tos_runtime::capture_begin();
        run_source(
            "layout.HC",
            r#"
class Vec
{
  I16 x;
  U8 tag;
};
class Obj
{
  U8 lead;
  Vec pos;
  U8 samples[3];
};
U0 Main()
{
  Obj o;
  o.pos.x=-2;
  o.pos.tag=255;
  o.samples[1]=42;
  "%d %d %d %d %d\n",o.pos.x,o.pos.tag,o.samples[1],
        offset(Obj.pos),sizeof(Obj);
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"-2 255 42 1 7\n");
    }

    #[test]
    fn union_fields_overlap() {
        tos_runtime::capture_begin();
        run_source(
            "union.HC",
            r#"
union Word
{
  U32 whole;
  U8 lo;
};
U0 Main()
{
  Word w;
  w.whole=258;
  "%d %d\n",w.lo,sizeof(Word);
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"2 4\n");
    }

    #[test]
    fn heap_msize_and_bits() {
        tos_runtime::capture_begin();
        run_source(
            "heap.HC",
            r#"
U0 Main()
{
  U8 *p=MAlloc(10);
  "%d\n",MSize(p);
  Bts(p,0);
  "%d\n",Bt(p,0);
  Free(p);
}
Main;
"#,
        )
        .unwrap();
        let out = String::from_utf8(tos_runtime::capture_take().unwrap()).unwrap();
        let mut lines = out.lines();
        let sz: i64 = lines.next().unwrap().parse().unwrap();
        assert!(sz >= 10);
        assert_eq!(lines.next().unwrap(), "1");
    }

    #[test]
    fn addressable_scalars_and_typed_indexing() {
        tos_runtime::capture_begin();
        run_source(
            "places.HC",
            r#"
U0 Main()
{
  I64 value=3;
  I64 *value_ptr=&value;
  U8 bytes[2];
  U8 *byte_ptr=bytes;
  *value_ptr=9;
  byte_ptr[1]=255;
  "%d %d\n",value,bytes[1];
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"9 255\n");
    }

    #[test]
    fn aggregate_initializers_and_multi_declarations() {
        tos_runtime::capture_begin();
        run_source(
            "init.HC",
            r#"
class Pair
{
  I16 x,y;
};
U0 Main()
{
  U8 a[3]={1,2,3},b[2]={4,5};
  Pair p={6,7};
  "%d %d %d %d\n",a[2],b[1],p.x,p.y;
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"3 5 6 7\n");
    }

    #[test]
    fn globals_are_shared_with_functions() {
        tos_runtime::capture_begin();
        run_source(
            "globals.HC",
            r#"
I64 counter=5;
U8 values[2]={10,20},other=3;
class GlobalPoint
{
  I64 x;
} point;
U0 Bump()
{
  counter++;
  values[1]+=2;
  point.x=9;
}
U0 Main()
{
  Bump;
  "%d %d %d %d\n",counter,values[1],other,point.x;
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"6 22 3 9\n");
    }

    #[test]
    fn circular_queue() {
        tos_runtime::capture_begin();
        run_source(
            "que.HC",
            r#"
class Node
{
  Node *next,*last;
  I64 val;
};
U0 Main()
{
  Node head;
  Node a;
  QueInit(&head);
  a.val=7;
  QueIns(&a,&head);
  Node *n=head.next;
  "%d\n",n->val;
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"7\n");
    }

    #[test]
    fn fs_pix_width() {
        tos_runtime::capture_begin();
        run_source(
            "fs.HC",
            r#"
U0 Main()
{
  "%d\n",Fs->pix_width;
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"640\n");
    }
}
