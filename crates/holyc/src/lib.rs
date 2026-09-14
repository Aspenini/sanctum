use holyc_codegen::{CodegenError, JitProgram, compile_jit};
use holyc_parse::Parser;
use holyc_sema::Sema;
use holyc_syntax::{PreprocessOpts, Session, SyntaxError, TokenKind, lex_buffer, preprocess};
use std::collections::HashMap;
use std::path::Path;
use templeos_compat::doldoc as tos_doldoc;
use thiserror::Error;

#[cfg(test)]
use templeos_compat::{host as tos_host, runtime as tos_runtime};

#[derive(Debug, Error)]
pub enum HolyCError {
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

pub fn compile_source(path: &str, src: &str) -> Result<JitProgram, HolyCError> {
    let mut session = Session::new();
    let tokens = lex_buffer(&mut session, path, src)?;
    let mut parser = Parser::new(&tokens, path, src);
    let mut ast = parser.parse_module()?;
    let mut sema = Sema::new(path, src);
    sema.run(&mut ast)?;
    let syms = templeos_compat::jit_symbols();
    let refs: Vec<(&str, *const u8)> = syms.iter().map(|(n, p)| (*n, *p)).collect();
    Ok(compile_jit(&ast, &sema, &refs, &HashMap::new())?)
}

pub fn compile_file(path: &Path, opts: &CompileOptions) -> Result<JitProgram, HolyCError> {
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
    let syms = templeos_compat::jit_symbols();
    let refs: Vec<(&str, *const u8)> = syms.iter().map(|(n, p)| (*n, *p)).collect();
    let mut expected_by_file: HashMap<u32, Vec<i64>> = HashMap::new();
    for token in &tokens {
        let idx = match token.kind {
            TokenKind::InsBin { idx } | TokenKind::InsBinSize { idx } => idx,
            _ => continue,
        };
        let indices = expected_by_file.entry(token.span.file).or_default();
        if !indices.contains(&idx) {
            indices.push(idx);
        }
    }
    let mut binary_resources = HashMap::new();
    for file in &session.files {
        let Some(expected) = expected_by_file.get(&file.id.0) else {
            continue;
        };
        for bin in tos_doldoc::parse_embedded_bins(&file.binary_tail, expected) {
            binary_resources.insert((file.id.0, bin.idx), bin.bytes);
        }
    }
    Ok(compile_jit(&ast, &sema, &refs, &binary_resources)?)
}

pub fn run_source(path: &str, src: &str) -> Result<(), HolyCError> {
    let prog = compile_source(path, src)?;
    run_program(prog, templeos_compat::HostMode::Headless)
}

pub fn run_file(path: &Path, opts: &CompileOptions) -> Result<(), HolyCError> {
    let prog = compile_file(path, opts)?;
    bind_file_roots(path, opts);
    run_program(prog, templeos_compat::HostMode::Headless)
}

pub fn run_file_interactive(path: &Path, opts: &CompileOptions) -> Result<(), HolyCError> {
    let prog = compile_file(path, opts)?;
    bind_file_roots(path, opts);
    run_program(prog, templeos_compat::HostMode::NativeWindow)
}

fn bind_file_roots(path: &Path, opts: &CompileOptions) {
    templeos_compat::host::set_file_roots(
        path.parent().map(Path::to_path_buf),
        opts.system_root.clone(),
    );
}

/// Run a compiled program using the selected compatibility host.
///
/// Interactive JIT modules remain mapped because cooperative background tasks
/// park inside generated code during shutdown. Sanctum isolates such runs in a
/// child process, so all mappings are reclaimed when that process exits.
pub fn run_program(
    mut prog: JitProgram,
    mode: templeos_compat::HostMode,
) -> Result<(), HolyCError> {
    templeos_compat::prepare_with_mode(mode);
    if let Err(error) = bind_program_globals(&mut prog) {
        templeos_compat::shutdown();
        return Err(error);
    }
    let result = prog.run().map_err(HolyCError::from);
    templeos_compat::shutdown();
    if mode != templeos_compat::HostMode::Headless {
        std::mem::forget(prog);
    }
    result
}

fn bind_program_globals(program: &mut JitProgram) -> Result<(), HolyCError> {
    let globals = program.global_bindings()?;
    let bindings = globals
        .iter()
        .map(|global| templeos_compat::GlobalBinding {
            name: &global.name,
            address: global.address,
            size: global.size,
        })
        .collect::<Vec<_>>();
    templeos_compat::bind_globals(&bindings);
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
    fn registry_source_updates_bound_jit_globals() {
        tos_runtime::capture_begin();
        run_source(
            "registry.HC",
            r#"
RegDft("Tests/Score", "F64 saved_score=4.25;\n");
RegExe("Tests/Score");
"%0.2f ", saved_score;
RegWrite("Tests/Score", "F64 saved_score=%0.2f;\n", 7.5);
saved_score=0.0;
RegExe("Tests/Score");
"%0.2f\n", saved_score;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"4.25 7.50\n");
    }

    #[test]
    fn templeos_shell_calls_accept_explicit_optional_arguments() {
        run_source(
            "shell_calls.HC",
            r#"
U0 Main()
{
  SettingsPush(NULL,0);
  MenuPush("File { Exit; }");
  AutoComplete(ON);
  WinBorder(ON,NULL);
  WinMax(NULL);
  DocCursor(ON,NULL);
  DocClear(NULL,FALSE);
  MenuPop;
  SettingsPop(NULL,0);
}
Main;
"#,
        )
        .unwrap();
    }

    #[test]
    fn default_device_context_transform_is_callable_from_holyc() {
        tos_runtime::capture_begin();
        run_source(
            "dc_transform.HC",
            r#"
U0 Main()
{
  CDC *dc=DCNew(8,8,NULL,FALSE);
  I64 x=1,y=2,z=3;
  dc->x=10;
  dc->y=20;
  dc->z=30;
  DCTransform(dc,&x,&y,&z);
  "%d %d %d\n",x,y,z;
  DCDel(dc);
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"11 22 33\n");
    }

    #[test]
    fn three_dimensional_symmetry_and_arrows_are_callable_from_holyc() {
        tos_runtime::capture_begin();
        run_source(
            "graphics_3d_calls.HC",
            r#"
U0 Main()
{
  CDC *dc=DCNew(32,32,NULL,FALSE);
  dc->color=15;
  I64 symmetry=DCSymmetry3Set(dc,0,0,0,1,0,0,0,1,0);
  I64 changed=GrArrow3(dc,2,16,0,24,16,0,2.75,1,0);
  "%d %d\n",symmetry,changed>0;
  DCDel(dc);
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"1 1\n");
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
  do {
    s++;
  } while (s<30);
  "%d\n",s;
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"30\n");
    }

    #[test]
    fn switch_start_end_runs_shared_porches() {
        tos_runtime::capture_begin();
        run_source(
            "sub_switch.HC",
            r#"
U0 Main()
{
  I64 i,total=0;
  for (i=0;i<4;i++)
    switch (i) {
      case 0: total+=1; break;
      start:
        total+=10;
        case 1: total+=100;  break;
        case 2: total+=1000; break;
      end:
        total+=10000;
        break;
    }
  "%d\n",total;
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"21121\n");
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
    fn register_hint_between_type_and_pointer() {
        run_source(
            "reg.HC",
            "class Node { I64 value; }; U0 Main() { Node reg *p=NULL; if (!p) return; } Main;",
        )
        .unwrap();
    }

    #[test]
    fn calls_callback_stored_in_class_field() {
        tos_runtime::capture_begin();
        run_source(
            "callback.HC",
            r#"
U0 Move(CDC *dc,I64 *x,I64 *y,I64 *z) { (*x)++; (*y)+=2; (*z)+=3; }
U0 Main() {
  CDC dc;
  I64 x=1,y=2,z=3;
  dc.transform=&Move;
  (*dc.transform)(&dc,&x,&y,&z);
  "%d %d %d\n",x,y,z;
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"2 4 6\n");
    }

    #[test]
    fn integer_lane_views_are_addressable() {
        tos_runtime::capture_begin();
        run_source(
            "lanes.HC",
            r#"
U0 Main() {
  I64 packed=0;
  packed.u8[0]=17;
  packed.u8[1]=34;
  packed.i32[1]=-2;
  "%d %d %d\n",packed.u8[0],packed.u8[1],packed.i32[1];
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"17 34 -2\n");
    }

    #[test]
    fn logical_operators_short_circuit_rhs_evaluation() {
        tos_runtime::capture_begin();
        run_source(
            "short_circuit.HC",
            r#"
class Thing { I64 value; };
I64 calls=0;
I64 Hit() { calls++; return 1; }
U0 Main() {
  Thing *p=NULL;
  if (p && p->value) calls=99;
  if (0 && Hit) calls=98;
  if (1 || Hit) calls+=0;
  "%d\n",calls;
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"0\n");
    }

    #[test]
    fn typed_pointer_arithmetic_uses_element_stride() {
        tos_runtime::capture_begin();
        run_source(
            "pointer_arithmetic.HC",
            r#"
U0 Main() {
  I64 values[4]={10,20,30,40};
  I64 *p=&values[0];
  "%d ",*(p+2);
  p++;
  "%d ",*p;
  p+=2;
  "%d ",*p;
  p--;
  "%d %d\n",*p,p-&values[0];
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"30 20 40 30 2\n");
    }

    #[test]
    fn talons_compiles_initializes_and_reaches_input() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = root.join("TempleOS/Demo/Games/Talons.HC");
        if !source.exists() {
            return;
        }
        let opts = CompileOptions {
            system_root: Some(root.join("TempleOS")),
            ..CompileOptions::default()
        };
        tos_runtime::capture_begin();
        run_file(&source, &opts).unwrap();
        let output = tos_runtime::capture_take().unwrap();
        assert!(
            output
                .windows(b"Initializing...".len())
                .any(|part| part == b"Initializing...")
        );
        assert_eq!(tos_host::frames_presented(), 12);
        let framebuffer = tos_host::framebuffer_rgb();
        assert_eq!(
            framebuffer.len(),
            tos_host::DEFAULT_WIDTH as usize * tos_host::DEFAULT_HEIGHT as usize * 3
        );
        assert!(framebuffer.chunks_exact(3).any(|pixel| pixel != [0, 0, 0]));
        let colors = framebuffer
            .chunks_exact(3)
            .collect::<std::collections::HashSet<_>>();
        assert!(
            colors.len() >= 5,
            "expected terrain, HUD, and sprite colors; got {colors:?}"
        );
        let water_pixels = framebuffer
            .chunks_exact(3)
            .filter(|pixel| *pixel == [0x00, 0x00, 0xAA])
            .count();
        assert!(
            water_pixels > 1_000,
            "expected a substantial rendered water/terrain polygon, got {water_pixels} blue pixels"
        );
    }

    #[test]
    fn mixed_float_arithmetic_and_comparisons() {
        tos_runtime::capture_begin();
        run_source(
            "float.HC",
            r#"
F64 Calc()
{
  F64 x=1.5;
  x+=2;
  return -x*2+8;
}
U0 Main()
{
  F64 value=Calc;
  if (0.5<value<2.0)
    "%d\n",ToI64(value);
}
Main;
"#,
        )
        .unwrap();
        let out = tos_runtime::capture_take().unwrap();
        assert_eq!(out, b"1\n");
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
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/holyc/inc_main.HC");
        run_file(&path, &CompileOptions::default()).unwrap();
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
    fn module_globals_are_contiguous_in_source_order() {
        tos_runtime::capture_begin();
        run_source(
            "global_layout.HC",
            r#"
I64 first[2]={11,22};
I64 second[2]={33,44};
"%d %d\n",first[2],&second[0]-&first[0];
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"33 2\n");
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

    #[test]
    fn integer_and_float_power_use_real_exponents() {
        tos_runtime::capture_begin();
        run_source(
            "pow.HC",
            r#"
U0 Main()
{
  "%d %d %d ",2`8,3`3,(-2)`3;
  "%d\n",ToI64(2.0`3.0);
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"256 27 -8 8\n");
    }

    #[test]
    fn postfix_cast_bitcasts_f64_to_i64() {
        tos_runtime::capture_begin();
        run_source(
            "postfix_cast.HC",
            r#"
U0 Main()
{
  "%d %d %d\n",(π/32)(I64)!=0,TEXT_ROWS,CH_SPACE;
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"1 60 32\n");
    }

    #[test]
    fn print_repeats_aux_format_chars() {
        tos_runtime::capture_begin();
        run_source(
            "print_h.HC",
            r#"
U0 Main()
{
  "%h*c",3,'x';
  "\n";
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"xxx\n");
    }

    #[test]
    fn line_invokes_the_plot_callback() {
        tos_runtime::capture_begin();
        run_source(
            "line.HC",
            r#"
I64 n=0;
Bool Plot(U8 *,I64,I64,I64)
{
  n++;
  return TRUE;
}
U0 Main()
{
  Line(NULL,0,0,0,2,0,0,&Plot);
  "%d\n",n;
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"3\n");
    }

    #[test]
    fn goto_jumps_to_labels() {
        tos_runtime::capture_begin();
        run_source(
            "goto.HC",
            r#"
U0 Main()
{
  I64 x=0;
  goto skip;
  x=1;
skip:
  x+=2;
  "%d\n",x;
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"2\n");
    }

    #[test]
    fn try_catch_handles_throw_from_a_callee() {
        tos_runtime::capture_begin();
        run_source(
            "except.HC",
            r#"
U0 Boom()
{
  throw(7);
}
U0 Main()
{
  I64 x=1;
  try {
    Boom;
    x=99;
  } catch
    x+=10;
  "%d\n",x;
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"11\n");
    }

    #[test]
    fn bare_defaulted_callee_is_a_call_in_a_declaration() {
        tos_runtime::capture_begin();
        run_source(
            "alias_decl.HC",
            r#"
U0 Main()
{
  CDC *dc=DCAlias;
  "%d\n",dc!=NULL;
  DCDel(dc);
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"1\n");
    }

    #[test]
    fn default_circle_and_fill_arguments_are_callable() {
        tos_runtime::capture_begin();
        run_source(
            "circle.HC",
            r#"
U0 Main()
{
  CDC *dc=DCNew(48,48,NULL,FALSE);
  dc->color=15;
  dc->thick=2;
  I64 changed=GrCircle3(dc,24,24,0,10);
  "%d\n",changed>0;
  DCDel(dc);
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"1\n");
    }

    #[test]
    fn nested_function_reads_and_writes_outer_locals() {
        tos_runtime::capture_begin();
        run_source(
            "nested.HC",
            r#"
U0 Main()
{
  I64 n=3;
  I64 Add()
  {
    return n+1;
  }
  U0 Bump()
  {
    n++;
  }
  Bump;
  "%d %d\n",Add,n;
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"5 4\n");
    }

    #[test]
    fn nested_function_pointer_keeps_outer_locals() {
        tos_runtime::capture_begin();
        run_source(
            "nested_ptr.HC",
            r#"
U0 Main()
{
  I64 n=7;
  I64 Get()
  {
    return n;
  }
  "%d\n",(*&Get)();
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"7\n");
    }

    #[test]
    fn nested_function_sees_grandparent_locals() {
        tos_runtime::capture_begin();
        run_source(
            "nested_depth.HC",
            r#"
U0 Main()
{
  I64 a=10;
  U0 Mid()
  {
    I64 b=2;
    I64 Inner()
    {
      return a+b;
    }
    "%d\n",Inner;
  }
  Mid;
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"12\n");
    }

    #[test]
    fn file_read_find_and_cd_stay_inside_project_root() {
        let root = std::env::temp_dir().join(format!("sanctum-holyc-fs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("note.TXT"), b"hello file").unwrap();
        std::fs::write(root.join("sub").join("inner.HC"), b"inner").unwrap();
        tos_host::set_file_roots(Some(root.clone()), None);
        tos_runtime::capture_begin();
        run_source(
            "files.HC",
            r#"
U0 Main()
{
  I64 size=0;
  U8 *s=FileRead("note.TXT",&size);
  "%s %d ",s,size;
  Free(s);
  CDirEntry de;
  "%d ",FileFind("note.TXT",&de);
  "%d ",Cd("sub");
  s=FileRead("inner.HC");
  "%s ",s;
  Free(s);
  Free(de.full_name);
  "%d\n",FileRead("../secret.TXT")==NULL;
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(
            tos_runtime::capture_take().unwrap(),
            b"hello file 10 1 1 inner 1\n"
        );
        let _ = std::fs::remove_dir_all(root);
        tos_host::set_file_roots(None, None);
    }

    #[test]
    fn file_read_opens_templeos_system_paths() {
        let templeos = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../TempleOS");
        let tic = templeos.join("Demo/Games/TicTacToe.HC");
        if !tic.is_file() {
            eprintln!("skipping ::/ FileRead test: {} is absent", tic.display());
            return;
        }
        tos_host::set_file_roots(None, Some(templeos));
        tos_runtime::capture_begin();
        run_source(
            "system_file.HC",
            r#"
U0 Main()
{
  I64 size=0;
  U8 *s=FileRead("::/Demo/Games/TicTacToe.HC",&size);
  "%d %d\n",s!=NULL,size>20;
  Free(s);
}
Main;
"#,
        )
        .unwrap();
        assert_eq!(tos_runtime::capture_take().unwrap(), b"1 1\n");
        tos_host::set_file_roots(None, None);
    }
}
