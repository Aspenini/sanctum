//! Name resolution and light type checking.

use holyc_ast::*;
use holyc_syntax::{Span, SyntaxError};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct FunctionInfo {
    pub name: String,
    pub ret: Ty,
    pub params: Vec<(String, Ty)>,
    pub variadic: bool,
    pub is_builtin: bool,
    pub link_name: String,
    pub parent_link: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MemberInfo {
    pub name: String,
    pub ty: Ty,
    pub offset: i64,
    pub size: i64,
}

/// Packed class layout (TempleOS: `offset = running size; size += member_size`).
#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub name: String,
    pub size: i64,
    pub union_: bool,
    pub members: Vec<MemberInfo>,
}

impl ClassInfo {
    pub fn member(&self, name: &str) -> Option<&MemberInfo> {
        self.members.iter().find(|m| m.name == name)
    }
}

pub struct Sema {
    pub functions: HashMap<String, FunctionInfo>,
    pub globals: HashMap<String, Ty>,
    pub classes: HashMap<String, ClassInfo>,
    errors: Vec<SyntaxError>,
    path: String,
    src: String,
    current_link: String,
}

impl Sema {
    pub fn new(path: &str, src: &str) -> Self {
        let mut s = Self {
            functions: HashMap::new(),
            globals: HashMap::new(),
            classes: HashMap::new(),
            errors: Vec::new(),
            path: path.into(),
            src: src.into(),
            current_link: String::new(),
        };
        s.add_builtin(
            "Print",
            Ty::U0,
            vec![("fmt", Ty::Ptr(Box::new(Ty::U8)))],
            true,
        );
        s.add_builtin("PutChars", Ty::U0, vec![("ch", Ty::I64)], false);
        s.add_builtin("ToI64", Ty::I64, vec![("x", Ty::F64)], false);
        s.add_builtin("ToF64", Ty::F64, vec![("x", Ty::I64)], false);
        s.add_builtin("ToBool", Ty::I64, vec![("x", Ty::I64)], false);
        s.add_builtin(
            "MAlloc",
            Ty::Ptr(Box::new(Ty::U8)),
            vec![("size", Ty::I64), ("task", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin(
            "CAlloc",
            Ty::Ptr(Box::new(Ty::U8)),
            vec![("size", Ty::I64), ("task", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin(
            "Free",
            Ty::U0,
            vec![("ptr", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin(
            "StrLen",
            Ty::I64,
            vec![("s", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin(
            "MemCpy",
            Ty::Ptr(Box::new(Ty::U8)),
            vec![
                ("dst", Ty::Ptr(Box::new(Ty::U8))),
                ("src", Ty::Ptr(Box::new(Ty::U8))),
                ("n", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "MemSet",
            Ty::Ptr(Box::new(Ty::U8)),
            vec![
                ("dst", Ty::Ptr(Box::new(Ty::U8))),
                ("val", Ty::I64),
                ("n", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "MSize",
            Ty::I64,
            vec![("ptr", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin(
            "QueInit",
            Ty::U0,
            vec![("head", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin(
            "QueIns",
            Ty::U0,
            vec![
                ("entry", Ty::Ptr(Box::new(Ty::U8))),
                ("pred", Ty::Ptr(Box::new(Ty::U8))),
            ],
            false,
        );
        s.add_builtin(
            "QueRem",
            Ty::U0,
            vec![("entry", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin(
            "QueDel",
            Ty::U0,
            vec![
                ("head", Ty::Ptr(Box::new(Ty::U8))),
                ("remove_first", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "Bt",
            Ty::I64,
            vec![("field", Ty::Ptr(Box::new(Ty::U8))), ("bit", Ty::I64)],
            false,
        );
        s.add_builtin(
            "Bts",
            Ty::I64,
            vec![("field", Ty::Ptr(Box::new(Ty::U8))), ("bit", Ty::I64)],
            false,
        );
        s.add_builtin(
            "Btr",
            Ty::I64,
            vec![("field", Ty::Ptr(Box::new(Ty::U8))), ("bit", Ty::I64)],
            false,
        );
        s.add_builtin(
            "LBts",
            Ty::I64,
            vec![("field", Ty::Ptr(Box::new(Ty::U8))), ("bit", Ty::I64)],
            false,
        );
        s.add_builtin(
            "LBtr",
            Ty::I64,
            vec![("field", Ty::Ptr(Box::new(Ty::U8))), ("bit", Ty::I64)],
            false,
        );
        s.add_builtin("tS", Ty::F64, vec![], false);
        s.add_builtin("Jiffies", Ty::I64, vec![], false);
        s.add_builtin("Blink", Ty::I64, vec![], false);
        s.add_builtin("Rand", Ty::F64, vec![], false);
        s.add_builtin("RandU16", Ty::I64, vec![], false);
        s.add_builtin("RandU32", Ty::I64, vec![], false);
        s.add_builtin("RandI16", Ty::I64, vec![], false);
        s.add_builtin("RandI64", Ty::I64, vec![], false);
        s.add_builtin("Abs", Ty::I64, vec![("x", Ty::I64)], false);
        s.add_builtin("SqrI64", Ty::I64, vec![("x", Ty::I64)], false);
        s.add_builtin(
            "ClampI64",
            Ty::I64,
            vec![("x", Ty::I64), ("lo", Ty::I64), ("hi", Ty::I64)],
            false,
        );
        s.add_builtin(
            "Wrap",
            Ty::F64,
            vec![("a", Ty::F64), ("base", Ty::F64)],
            false,
        );
        let i64_ptr = Ty::Ptr(Box::new(Ty::I64));
        let cd3_ptr = Ty::Ptr(Box::new(Ty::Class {
            name: "CD3".into(),
            size: 24,
        }));
        s.add_builtin(
            "Mat4x4IdentEqu",
            i64_ptr.clone(),
            vec![("r", i64_ptr.clone())],
            false,
        );
        s.add_builtin("Mat4x4IdentNew", i64_ptr.clone(), vec![], false);
        for name in ["Mat4x4RotX", "Mat4x4RotZ", "Mat4x4Scale"] {
            s.add_builtin(
                name,
                i64_ptr.clone(),
                vec![("r", i64_ptr.clone()), ("value", Ty::F64)],
                false,
            );
        }
        s.add_builtin(
            "Mat4x4MulXYZ",
            Ty::U0,
            vec![
                ("r", i64_ptr.clone()),
                ("x", i64_ptr.clone()),
                ("y", i64_ptr.clone()),
                ("z", i64_ptr.clone()),
            ],
            false,
        );
        s.add_builtin(
            "Mat4x4TranslationEqu",
            i64_ptr.clone(),
            vec![
                ("r", i64_ptr.clone()),
                ("x", Ty::I64),
                ("y", Ty::I64),
                ("z", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "D3Sub",
            cd3_ptr.clone(),
            vec![
                ("dst", cd3_ptr.clone()),
                ("lhs", cd3_ptr.clone()),
                ("rhs", cd3_ptr.clone()),
            ],
            false,
        );
        s.add_builtin(
            "D3NormSqr",
            Ty::F64,
            vec![("value", cd3_ptr.clone())],
            false,
        );
        s.add_builtin("D3Unit", cd3_ptr.clone(), vec![("value", cd3_ptr)], false);
        s.add_builtin(
            "SwapI64",
            Ty::U0,
            vec![("lhs", i64_ptr.clone()), ("rhs", i64_ptr.clone())],
            false,
        );
        for name in ["Sin", "Cos", "Sqrt", "ACos"] {
            s.add_builtin(name, Ty::F64, vec![("value", Ty::F64)], false);
        }
        for name in ["Min", "Max"] {
            s.add_builtin(name, Ty::F64, vec![("a", Ty::F64), ("b", Ty::F64)], false);
        }
        s.add_builtin(
            "Clamp",
            Ty::F64,
            vec![("value", Ty::F64), ("lo", Ty::F64), ("hi", Ty::F64)],
            false,
        );
        s.add_builtin("Sign", Ty::F64, vec![("value", Ty::F64)], false);
        s.add_builtin("Sleep", Ty::U0, vec![("ms", Ty::I64)], false);
        s.add_builtin("Yield", Ty::U0, vec![], false);
        s.add_builtin("Fs", Ty::Ptr(Box::new(Ty::U8)), vec![], false);
        s.add_builtin("Gs", Ty::Ptr(Box::new(Ty::U8)), vec![], false);
        s.add_builtin("mp_cnt", Ty::I64, vec![], false);
        let task_ptr = Ty::Ptr(Box::new(Ty::Class {
            name: "CTask".into(),
            size: 72,
        }));
        s.add_builtin(
            "Spawn",
            task_ptr.clone(),
            vec![
                ("fp_start_addr", Ty::Ptr(Box::new(Ty::U8))),
                ("data", Ty::Ptr(Box::new(Ty::U8))),
                ("task_name", Ty::Ptr(Box::new(Ty::U8))),
                ("target_cpu", Ty::I64),
                ("parent", task_ptr.clone()),
                ("stk_size", Ty::I64),
                ("flags", Ty::I64),
            ],
            false,
        );
        s.add_builtin("SndTaskEndCB", Ty::U0, vec![], false);
        s.add_builtin(
            "Beep",
            Ty::U0,
            vec![("ona", Ty::I64), ("busy", Ty::I64)],
            false,
        );
        s.add_builtin("Snd", Ty::U0, vec![("ona", Ty::I64)], false);
        s.add_builtin(
            "Play",
            Ty::U0,
            vec![
                ("song", Ty::Ptr(Box::new(Ty::U8))),
                ("words", Ty::Ptr(Box::new(Ty::U8))),
            ],
            false,
        );
        s.add_builtin("MusicSettingsRst", Ty::U0, vec![], false);
        s.add_builtin(
            "RegDft",
            Ty::I64,
            vec![
                ("path", Ty::Ptr(Box::new(Ty::U8))),
                ("defaults", Ty::Ptr(Box::new(Ty::U8))),
            ],
            false,
        );
        s.add_builtin(
            "RegExe",
            Ty::I64,
            vec![("path", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin(
            "RegWrite",
            Ty::I64,
            vec![
                ("path", Ty::Ptr(Box::new(Ty::U8))),
                ("fmt", Ty::Ptr(Box::new(Ty::U8))),
                ("value", Ty::F64),
            ],
            false,
        );
        for name in ["Refresh", "MenuPop", "PutExcept", "Exit"] {
            s.add_builtin(name, Ty::U0, vec![], false);
        }
        s.add_builtin(
            "SettingsPush",
            Ty::Ptr(Box::new(Ty::U8)),
            vec![("task", task_ptr.clone()), ("flags", Ty::I64)],
            false,
        );
        s.add_builtin(
            "SettingsPop",
            Ty::U0,
            vec![("task", task_ptr.clone()), ("flags", Ty::I64)],
            false,
        );
        s.add_builtin(
            "MenuPush",
            Ty::Ptr(Box::new(Ty::U8)),
            vec![("menu", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin("AutoComplete", Ty::I64, vec![("enabled", Ty::I64)], false);
        s.add_builtin(
            "WinBorder",
            Ty::I64,
            vec![("enabled", Ty::I64), ("task", task_ptr.clone())],
            false,
        );
        s.add_builtin("WinMax", Ty::U0, vec![("task", task_ptr.clone())], false);
        s.add_builtin(
            "DocCursor",
            Ty::I64,
            vec![("show", Ty::I64), ("doc", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin(
            "DocClear",
            Ty::U0,
            vec![("doc", Ty::Ptr(Box::new(Ty::U8))), ("clear_holds", Ty::I64)],
            false,
        );
        s.add_builtin(
            "ScanKey",
            Ty::I64,
            vec![
                ("ch", Ty::Ptr(Box::new(Ty::I64))),
                ("scan_code", Ty::Ptr(Box::new(Ty::I64))),
                ("echo", Ty::I64),
            ],
            false,
        );
        let cdc_ptr = Ty::Ptr(Box::new(Ty::Class {
            name: "CDC".into(),
            size: 64,
        }));
        let u8_ptr = Ty::Ptr(Box::new(Ty::U8));
        let i32_ptr = Ty::Ptr(Box::new(Ty::I32));
        s.add_builtin(
            "DCNew",
            cdc_ptr.clone(),
            vec![
                ("width", Ty::I64),
                ("height", Ty::I64),
                ("task", task_ptr.clone()),
                ("null_bitmap", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "DCAlias",
            cdc_ptr.clone(),
            vec![("dc", cdc_ptr.clone()), ("task", task_ptr.clone())],
            false,
        );
        s.add_builtin("DCDel", Ty::U0, vec![("dc", cdc_ptr.clone())], false);
        s.add_builtin(
            "DCFill",
            Ty::U0,
            vec![("dc", cdc_ptr.clone()), ("color", Ty::I64)],
            false,
        );
        for name in ["DCDepthBufAlloc", "DCDepthBufRst"] {
            s.add_builtin(name, i32_ptr.clone(), vec![("dc", cdc_ptr.clone())], false);
        }
        s.add_builtin(
            "DCMat4x4Set",
            Ty::U0,
            vec![("dc", cdc_ptr.clone()), ("r", i64_ptr.clone())],
            false,
        );
        s.add_builtin(
            "DCTransform",
            Ty::U0,
            vec![
                ("dc", cdc_ptr.clone()),
                ("x", i64_ptr.clone()),
                ("y", i64_ptr.clone()),
                ("z", i64_ptr.clone()),
            ],
            false,
        );
        s.add_builtin(
            "DCSymmetrySet",
            Ty::I64,
            vec![
                ("dc", cdc_ptr.clone()),
                ("x1", Ty::I64),
                ("y1", Ty::I64),
                ("x2", Ty::I64),
                ("y2", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "DCSymmetry3Set",
            Ty::I64,
            vec![
                ("dc", cdc_ptr.clone()),
                ("x1", Ty::I64),
                ("y1", Ty::I64),
                ("z1", Ty::I64),
                ("x2", Ty::I64),
                ("y2", Ty::I64),
                ("z2", Ty::I64),
                ("x3", Ty::I64),
                ("y3", Ty::I64),
                ("z3", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "DCClipLine",
            Ty::I64,
            vec![
                ("dc", cdc_ptr.clone()),
                ("x1", i64_ptr.clone()),
                ("y1", i64_ptr.clone()),
                ("x2", i64_ptr.clone()),
                ("y2", i64_ptr.clone()),
                ("width", Ty::I64),
                ("height", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "GrLine3",
            Ty::I64,
            vec![
                ("dc", cdc_ptr.clone()),
                ("x1", Ty::I64),
                ("y1", Ty::I64),
                ("z1", Ty::I64),
                ("x2", Ty::I64),
                ("y2", Ty::I64),
                ("z2", Ty::I64),
                ("step", Ty::I64),
                ("start", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "GrCircle3",
            Ty::I64,
            vec![
                ("dc", cdc_ptr.clone()),
                ("cx", Ty::I64),
                ("cy", Ty::I64),
                ("cz", Ty::I64),
                ("radius", Ty::I64),
                ("step", Ty::I64),
                ("start_radians", Ty::F64),
                ("len_radians", Ty::F64),
            ],
            false,
        );
        s.add_builtin(
            "GrFillPoly3",
            Ty::I64,
            vec![
                ("dc", cdc_ptr.clone()),
                ("n", Ty::I64),
                (
                    "poly",
                    Ty::Ptr(Box::new(Ty::Class {
                        name: "CD3I32".into(),
                        size: 12,
                    })),
                ),
            ],
            false,
        );
        s.add_builtin(
            "GrArrow3",
            Ty::I64,
            vec![
                ("dc", cdc_ptr.clone()),
                ("x1", Ty::I64),
                ("y1", Ty::I64),
                ("z1", Ty::I64),
                ("x2", Ty::I64),
                ("y2", Ty::I64),
                ("z2", Ty::I64),
                ("width", Ty::F64),
                ("step", Ty::I64),
                ("start", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "GrBlot",
            Ty::I64,
            vec![
                ("dc", cdc_ptr.clone()),
                ("x", Ty::I64),
                ("y", Ty::I64),
                ("image", cdc_ptr.clone()),
            ],
            false,
        );
        s.add_builtin(
            "GrPrint",
            Ty::I64,
            vec![
                ("dc", cdc_ptr.clone()),
                ("x", Ty::I64),
                ("y", Ty::I64),
                ("fmt", u8_ptr.clone()),
            ],
            true,
        );
        s.add_builtin(
            "Sprite3",
            Ty::U0,
            vec![
                ("dc", cdc_ptr.clone()),
                ("x", Ty::I64),
                ("y", Ty::I64),
                ("z", Ty::I64),
                ("elems", u8_ptr.clone()),
                ("just_one_elem", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "Sprite3B",
            Ty::U0,
            vec![
                ("dc", cdc_ptr.clone()),
                ("x", Ty::I64),
                ("y", Ty::I64),
                ("z", Ty::I64),
                ("elems", u8_ptr.clone()),
            ],
            false,
        );
        s.add_builtin(
            "SpriteInterpolate",
            u8_ptr.clone(),
            vec![("t", Ty::F64), ("a", u8_ptr.clone()), ("b", u8_ptr.clone())],
            false,
        );
        s.add_builtin(
            "SpriteTransform",
            u8_ptr.clone(),
            vec![("elems", u8_ptr), ("r", i64_ptr.clone())],
            false,
        );
        s.add_builtin(
            "Tri",
            Ty::F64,
            vec![("t", Ty::F64), ("period", Ty::F64)],
            false,
        );
        s.add_builtin(
            "Saw",
            Ty::F64,
            vec![("t", Ty::F64), ("period", Ty::F64)],
            false,
        );
        // Minimal CTask / CCPU so `Fs->pix_width` type-checks. Offsets match tos-abi.
        s.classes.insert(
            "CTask".into(),
            ClassInfo {
                name: "CTask".into(),
                size: 72,
                union_: false,
                members: vec![
                    MemberInfo {
                        name: "addr".into(),
                        ty: Ty::Ptr(Box::new(Ty::Class {
                            name: "CTask".into(),
                            size: 72,
                        })),
                        offset: 0,
                        size: 8,
                    },
                    MemberInfo {
                        name: "pix_width".into(),
                        ty: Ty::I64,
                        offset: 8,
                        size: 8,
                    },
                    MemberInfo {
                        name: "pix_height".into(),
                        ty: Ty::I64,
                        offset: 16,
                        size: 8,
                    },
                    MemberInfo {
                        name: "draw_it".into(),
                        ty: Ty::Ptr(Box::new(Ty::U8)),
                        offset: 24,
                        size: 8,
                    },
                    MemberInfo {
                        name: "task_end_cb".into(),
                        ty: Ty::Ptr(Box::new(Ty::U8)),
                        offset: 32,
                        size: 8,
                    },
                    MemberInfo {
                        name: "song_task".into(),
                        ty: task_ptr.clone(),
                        offset: 40,
                        size: 8,
                    },
                    MemberInfo {
                        name: "animate_task".into(),
                        ty: task_ptr.clone(),
                        offset: 48,
                        size: 8,
                    },
                    MemberInfo {
                        name: "pix_left".into(),
                        ty: Ty::I64,
                        offset: 56,
                        size: 8,
                    },
                    MemberInfo {
                        name: "pix_top".into(),
                        ty: Ty::I64,
                        offset: 64,
                        size: 8,
                    },
                ],
            },
        );
        s.classes.insert(
            "CCPU".into(),
            ClassInfo {
                name: "CCPU".into(),
                size: 16,
                union_: false,
                members: vec![
                    MemberInfo {
                        name: "num".into(),
                        ty: Ty::I64,
                        offset: 0,
                        size: 8,
                    },
                    MemberInfo {
                        name: "idle_factor".into(),
                        ty: Ty::F64,
                        offset: 8,
                        size: 8,
                    },
                ],
            },
        );
        s.classes.insert(
            "CQue".into(),
            packed_class(
                "CQue",
                vec![
                    (
                        "next",
                        Ty::Ptr(Box::new(Ty::Class {
                            name: "CQue".into(),
                            size: 16,
                        })),
                    ),
                    (
                        "last",
                        Ty::Ptr(Box::new(Ty::Class {
                            name: "CQue".into(),
                            size: 16,
                        })),
                    ),
                ],
            ),
        );
        s.classes.insert(
            "CD3".into(),
            packed_class("CD3", vec![("x", Ty::F64), ("y", Ty::F64), ("z", Ty::F64)]),
        );
        s.classes.insert(
            "CD3I32".into(),
            packed_class(
                "CD3I32",
                vec![("x", Ty::I32), ("y", Ty::I32), ("z", Ty::I32)],
            ),
        );
        s.classes.insert(
            "CD3I64".into(),
            packed_class(
                "CD3I64",
                vec![("x", Ty::I64), ("y", Ty::I64), ("z", Ty::I64)],
            ),
        );
        s.classes.insert(
            "CDC".into(),
            packed_class(
                "CDC",
                vec![
                    ("width", Ty::I32),
                    ("height", Ty::I32),
                    ("flags", Ty::I32),
                    ("color", Ty::U32),
                    ("r", Ty::Ptr(Box::new(Ty::I64))),
                    ("x", Ty::I32),
                    ("y", Ty::I32),
                    ("z", Ty::I32),
                    ("thick", Ty::I32),
                    ("transform", Ty::Ptr(Box::new(Ty::U8))),
                    ("body", Ty::Ptr(Box::new(Ty::U8))),
                    ("depth_buf", Ty::Ptr(Box::new(Ty::I32))),
                ],
            ),
        );
        s.classes.insert(
            "CWinMgrGlbls".into(),
            packed_class(
                "CWinMgrGlbls",
                vec![
                    ("updates", Ty::I64),
                    ("ode_time", Ty::F64),
                    ("last_ode_time", Ty::F64),
                    ("fps", Ty::F64),
                    ("ideal_refresh_tS", Ty::F64),
                    ("last_refresh_tS", Ty::F64),
                    ("t", Ty::Ptr(Box::new(Ty::U8))),
                    ("show_menu", Ty::I64),
                    ("grab_scroll", Ty::I64),
                    ("grab_scroll_closed", Ty::I64),
                ],
            ),
        );
        s.classes.insert(
            "CMusicGlbls".into(),
            packed_class(
                "CMusicGlbls",
                vec![
                    ("cur_song", Ty::Ptr(Box::new(Ty::U8))),
                    ("cur_song_task", task_ptr.clone()),
                    ("octave", Ty::I64),
                    ("note_len", Ty::F64),
                    ("note_map", Ty::Array(Box::new(Ty::U8), Some(7))),
                    ("mute", Ty::I64),
                    ("meter_top", Ty::I64),
                    ("meter_bottom", Ty::I64),
                    ("tempo", Ty::F64),
                    ("stacatto_factor", Ty::F64),
                    ("play_note_num", Ty::I64),
                    ("tM_correction", Ty::F64),
                    ("last_Beat", Ty::F64),
                    ("last_tM", Ty::F64),
                ],
            ),
        );
        s.globals.insert(
            "winmgr".into(),
            Ty::Class {
                name: "CWinMgrGlbls".into(),
                size: 80,
            },
        );
        s.globals.insert(
            "music".into(),
            Ty::Class {
                name: "CMusicGlbls".into(),
                size: 111,
            },
        );
        let d3i64 = Ty::Class {
            name: "CD3I64".into(),
            size: 24,
        };
        s.classes.insert(
            "CMsStateGlbls".into(),
            packed_class(
                "CMsStateGlbls",
                vec![
                    ("pos", d3i64.clone()),
                    ("pos_text", d3i64.clone()),
                    ("presnap", d3i64.clone()),
                    ("offset", d3i64),
                    (
                        "scale",
                        Ty::Class {
                            name: "CD3".into(),
                            size: 24,
                        },
                    ),
                    ("speed", Ty::F64),
                    ("timestamp", Ty::I64),
                    ("dbl_time", Ty::F64),
                    ("left_dbl_time", Ty::F64),
                    ("right_dbl_time", Ty::F64),
                    ("lb", Ty::U8),
                    ("rb", Ty::U8),
                    ("show", Ty::U8),
                    ("has_wheel", Ty::U8),
                    ("left_dbl", Ty::U8),
                    ("left_down_sent", Ty::U8),
                    ("right_dbl", Ty::U8),
                    ("right_down_sent", Ty::U8),
                ],
            ),
        );
        s.globals.insert(
            "ms".into(),
            Ty::Class {
                name: "CMsStateGlbls".into(),
                size: 168,
            },
        );
        if let Some(f) = s.functions.get_mut("Fs") {
            f.ret = Ty::Ptr(Box::new(Ty::Class {
                name: "CTask".into(),
                size: 72,
            }));
        }
        if let Some(f) = s.functions.get_mut("Gs") {
            f.ret = Ty::Ptr(Box::new(Ty::Class {
                name: "CCPU".into(),
                size: 16,
            }));
        }
        s
    }

    fn add_builtin(&mut self, name: &str, ret: Ty, params: Vec<(&str, Ty)>, variadic: bool) {
        self.functions.insert(
            name.into(),
            FunctionInfo {
                name: name.into(),
                ret,
                params: params
                    .into_iter()
                    .map(|(n, t)| (n.to_string(), t))
                    .collect(),
                variadic,
                is_builtin: true,
                link_name: format!("tos_{name}"),
                parent_link: None,
            },
        );
    }

    fn register_function(&mut self, function: &FnDecl, parent_link: Option<String>) {
        let link_name = match &parent_link {
            Some(parent) => Self::nested_link_name(parent, &function.name),
            None => function.name.clone(),
        };
        let ret = resolve_ty(&function.ret, &self.classes);
        let params = function
            .params
            .iter()
            .map(|p| (p.name.clone(), resolve_ty(&p.ty, &self.classes)))
            .collect();
        self.functions.insert(
            link_name.clone(),
            FunctionInfo {
                name: function.name.clone(),
                ret,
                params,
                variadic: function.variadic,
                is_builtin: false,
                link_name: link_name.clone(),
                parent_link,
            },
        );
        if let Some(body) = &function.body {
            for stmt in body {
                self.register_nested(stmt, &link_name);
            }
        }
    }

    fn register_nested(&mut self, stmt: &Stmt, parent_link: &str) {
        match stmt {
            Stmt::Fn(function) => self.register_function(function, Some(parent_link.to_string())),
            Stmt::Block { stmts, .. } | Stmt::Start { body: stmts, .. } => {
                for stmt in stmts {
                    self.register_nested(stmt, parent_link);
                }
            }
            Stmt::If { then, else_, .. } => {
                self.register_nested(then, parent_link);
                if let Some(else_) = else_ {
                    self.register_nested(else_, parent_link);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Switch { body, .. } => self.register_nested(body, parent_link),
            Stmt::Try { body, catch, .. } => {
                self.register_nested(body, parent_link);
                self.register_nested(catch, parent_link);
            }
            _ => {}
        }
    }

    pub fn nested_link_name(parent: &str, name: &str) -> String {
        format!("{parent}__{name}")
    }

    pub fn lookup_function(&self, name: &str) -> Option<&FunctionInfo> {
        self.lookup_function_from(name, &self.current_link)
    }

    pub fn lookup_function_from(&self, name: &str, from_link: &str) -> Option<&FunctionInfo> {
        let mut current = if from_link.is_empty() {
            None
        } else {
            Some(from_link.to_string())
        };
        while let Some(link) = current {
            let mangled = Self::nested_link_name(&link, name);
            if let Some(info) = self.functions.get(&mangled) {
                return Some(info);
            }
            current = self
                .functions
                .get(&link)
                .and_then(|info| info.parent_link.clone());
        }
        self.functions.get(name)
    }

    pub fn run(&mut self, module: &mut Module) -> Result<(), SyntaxError> {
        for item in &module.items {
            match item {
                Item::Fn(f) => self.register_function(f, None),
                Item::Class(c) => {
                    let info = layout_class(c, &self.classes);
                    self.classes.insert(c.name.clone(), info);
                }
                Item::Stmt(stmt) => collect_globals(stmt, &self.classes, &mut self.globals),
            }
        }
        for item in &module.items {
            match item {
                Item::Stmt(stmt) => collect_registry_globals(stmt, &mut self.globals),
                Item::Fn(function) => {
                    if let Some(body) = &function.body {
                        for stmt in body {
                            collect_registry_globals(stmt, &mut self.globals);
                        }
                    }
                }
                Item::Class(_) => {}
            }
        }
        for item in &mut module.items {
            if let Item::Stmt(stmt) = item {
                self.rewrite_stmt(stmt);
            }
            if let Item::Fn(f) = item {
                self.rewrite_function(f);
            }
        }
        if let Some(e) = self.errors.pop() {
            return Err(e);
        }
        Ok(())
    }

    fn rewrite_stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Expr { expr, span } => {
                self.rewrite_expr(expr);
                self.callify_function_ident(expr);
                let _ = span;
            }
            Stmt::Block { stmts, .. } => {
                for s in stmts {
                    self.rewrite_stmt(s);
                }
            }
            Stmt::If {
                cond, then, else_, ..
            } => {
                self.rewrite_expr(cond);
                self.rewrite_stmt(then);
                if let Some(e) = else_ {
                    self.rewrite_stmt(e);
                }
            }
            Stmt::While { cond, body, .. } | Stmt::DoWhile { cond, body, .. } => {
                self.rewrite_expr(cond);
                self.rewrite_stmt(body);
            }
            Stmt::For {
                init,
                cond,
                inc,
                body,
                ..
            } => {
                if let Some(i) = init {
                    self.rewrite_stmt(i);
                }
                if let Some(c) = cond {
                    self.rewrite_expr(c);
                }
                if let Some(i) = inc {
                    self.rewrite_expr(i);
                }
                self.rewrite_stmt(body);
            }
            Stmt::Return { expr, .. } => {
                if let Some(e) = expr {
                    self.rewrite_expr(e);
                }
            }
            Stmt::Switch { expr, body, .. } => {
                self.rewrite_expr(expr);
                self.rewrite_stmt(body);
            }
            Stmt::Case {
                value, range_end, ..
            } => {
                if let Some(value) = value {
                    self.rewrite_expr(value);
                }
                if let Some(range_end) = range_end {
                    self.rewrite_expr(range_end);
                }
            }
            Stmt::Start { body, .. } => {
                for s in body {
                    self.rewrite_stmt(s);
                }
            }
            Stmt::Try { body, catch, .. } => {
                self.rewrite_stmt(body);
                self.rewrite_stmt(catch);
            }
            Stmt::Throw { expr, .. } => self.rewrite_expr(expr),
            Stmt::Decl(v) => {
                if let Some(init) = &mut v.init {
                    self.rewrite_expr(init);
                    self.callify_function_ident(init);
                }
            }
            Stmt::Fn(function) => self.rewrite_function(function),
            _ => {}
        }
    }

    fn rewrite_function(&mut self, function: &mut FnDecl) {
        let Some(body) = &mut function.body else {
            return;
        };
        let link = if self.current_link.is_empty() {
            function.name.clone()
        } else {
            Self::nested_link_name(&self.current_link, &function.name)
        };
        let previous = std::mem::replace(&mut self.current_link, link);
        for stmt in body {
            self.rewrite_stmt(stmt);
        }
        self.current_link = previous;
    }

    fn callify_function_ident(&self, expr: &mut Expr) {
        let ExprKind::Ident(name) = &expr.kind else {
            return;
        };
        if self.lookup_function(name).is_some() {
            let callee = expr.clone();
            expr.kind = ExprKind::Call {
                callee: Box::new(callee),
                args: vec![],
            };
        }
    }

    fn rewrite_expr(&mut self, expr: &mut Expr) {
        if let ExprKind::Field { base, name, .. } = &expr.kind
            && name == "jiffies"
            && matches!(&base.kind, ExprKind::Ident(base_name) if base_name == "cnts")
        {
            expr.kind = ExprKind::Call {
                callee: Box::new(Expr {
                    span: expr.span,
                    kind: ExprKind::Ident("Jiffies".into()),
                }),
                args: vec![],
            };
            return;
        }
        match &mut expr.kind {
            ExprKind::Unary { expr, .. } | ExprKind::Deref(expr) | ExprKind::Cast { expr, .. } => {
                self.rewrite_expr(expr)
            }
            ExprKind::Addr(inner) => {
                if !matches!(&inner.kind, ExprKind::Ident(n) if self.lookup_function(n).is_some()) {
                    self.rewrite_expr(inner);
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                self.rewrite_expr(lhs);
                self.rewrite_expr(rhs);
                if op.is_assign() {
                    self.callify_function_ident(rhs);
                }
            }
            ExprKind::ChainCmp { first, rest } => {
                self.rewrite_expr(first);
                for (_, e) in rest {
                    self.rewrite_expr(e);
                }
            }
            ExprKind::Sequence(exprs) => {
                for expr in exprs {
                    self.rewrite_expr(expr);
                }
            }
            ExprKind::Call { callee, args } => {
                self.rewrite_expr(callee);
                for a in args.iter_mut().flatten() {
                    self.rewrite_expr(a);
                    self.callify_function_ident(a);
                }
            }
            ExprKind::Index { base, index } => {
                self.rewrite_expr(base);
                self.rewrite_expr(index);
            }
            ExprKind::InitList(values) => {
                for value in values {
                    self.rewrite_expr(value);
                }
            }
            ExprKind::Field { base, .. } => self.rewrite_expr(base),
            ExprKind::Ident(name) => {
                if name == "π" || name == "pi" {
                    expr.kind = ExprKind::Float(std::f64::consts::PI);
                } else if name == "∞" || name == "inf" {
                    expr.kind = ExprKind::Float(f64::INFINITY);
                } else if name == "TRUE" || name == "ON" || name == "true" {
                    expr.kind = ExprKind::Int(1);
                } else if name == "FALSE" || name == "OFF" || name == "false" || name == "NULL" {
                    expr.kind = ExprKind::Int(0);
                } else if name == "F64_MAX" {
                    expr.kind = ExprKind::Float(f64::MAX);
                } else if let Some(value) = builtin_integer_constant(name) {
                    expr.kind = ExprKind::Int(value);
                } else if self
                    .lookup_function(name)
                    .is_some_and(|function| function.params.is_empty())
                {
                    let callee = expr.clone();
                    expr.kind = ExprKind::Call {
                        callee: Box::new(callee),
                        args: vec![],
                    };
                }
            }
            _ => {}
        }
    }

    pub fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.errors
            .push(SyntaxError::at(&self.path, &self.src, span, msg));
    }
}

fn builtin_integer_constant(name: &str) -> Option<i64> {
    Some(match name {
        "BLACK" => 0,
        "BLUE" => 1,
        "GREEN" => 2,
        "CYAN" => 3,
        "RED" => 4,
        "PURPLE" => 5,
        "BROWN" => 6,
        "LTGRAY" => 7,
        "DKGRAY" => 8,
        "LTBLUE" => 9,
        "LTGREEN" => 10,
        "LTCYAN" => 11,
        "LTRED" => 12,
        "LTPURPLE" => 13,
        "YELLOW" => 14,
        "WHITE" => 15,
        "TRANSPARENT" => 0xff,
        "ROPF_DITHER" => 0x4000_0000,
        "DCF_NO_TRANSPARENTS" => 4,
        "DCF_TRANSFORMATION" => 0x100,
        "DCF_SYMMETRY" => 0x200,
        "SC_CURSOR_UP" => 0x48,
        "SC_CURSOR_DOWN" => 0x50,
        "SC_CURSOR_LEFT" => 0x4b,
        "SC_CURSOR_RIGHT" => 0x4d,
        "CH_ESC" => 0x1b,
        "CH_SHIFT_ESC" => 0x1c,
        "FONT_WIDTH" | "FONT_HEIGHT" => 8,
        "GR_WIDTH" => 640,
        "GR_HEIGHT" => 480,
        "JIFFY_FREQ" => 1000,
        "MP_PROCESSORS_NUM" => 128,
        "U16_MAX" => 0xffff,
        "I64_MAX" => i64::MAX,
        _ => return None,
    })
}

fn collect_globals(
    stmt: &Stmt,
    classes: &HashMap<String, ClassInfo>,
    out: &mut HashMap<String, Ty>,
) {
    match stmt {
        Stmt::Decl(var) => {
            out.insert(var.name.clone(), resolve_ty(&var.ty, classes));
        }
        Stmt::Block { stmts, .. } => {
            for stmt in stmts {
                collect_globals(stmt, classes, out);
            }
        }
        _ => {}
    }
}

fn collect_registry_globals(stmt: &Stmt, out: &mut HashMap<String, Ty>) {
    match stmt {
        Stmt::Expr { expr, .. } => collect_registry_globals_expr(expr, out),
        Stmt::Block { stmts, .. } | Stmt::Start { body: stmts, .. } => {
            for stmt in stmts {
                collect_registry_globals(stmt, out);
            }
        }
        Stmt::If { then, else_, .. } => {
            collect_registry_globals(then, out);
            if let Some(else_) = else_ {
                collect_registry_globals(else_, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Switch { body, .. } => collect_registry_globals(body, out),
        Stmt::Try { body, catch, .. } => {
            collect_registry_globals(body, out);
            collect_registry_globals(catch, out);
        }
        Stmt::Fn(function) => {
            if let Some(body) = &function.body {
                for stmt in body {
                    collect_registry_globals(stmt, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_registry_globals_expr(expr: &Expr, out: &mut HashMap<String, Ty>) {
    let ExprKind::Call { callee, args } = &expr.kind else {
        return;
    };
    if matches!(&callee.kind, ExprKind::Ident(name) if name == "RegDft")
        && let Some(Some(Expr {
            kind: ExprKind::Str(defaults),
            ..
        })) = args.get(1)
    {
        for statement in defaults.split(';') {
            let declaration = statement
                .split_once('=')
                .map_or(statement, |(left, _)| left);
            let mut words = declaration.split_whitespace();
            let (Some(kind), Some(name)) = (words.next(), words.next()) else {
                continue;
            };
            if let Some(ty) = Ty::from_builtin(kind)
                && !ty.is_void()
                && !ty.is_aggregate()
            {
                out.entry(name.to_string()).or_insert(ty);
            }
        }
    }
}

pub fn resolve_ty(t: &TypeRef, classes: &HashMap<String, ClassInfo>) -> Ty {
    match t {
        TypeRef::Name(n) => {
            if let Some(b) = Ty::from_builtin(n) {
                b
            } else if let Some(c) = classes.get(n) {
                Ty::Class {
                    name: n.clone(),
                    size: c.size,
                }
            } else {
                // Unknown named type: treat as a class of size 0 so `Foo *` still works.
                Ty::Class {
                    name: n.clone(),
                    size: 0,
                }
            }
        }
        TypeRef::Ptr(inner) => Ty::Ptr(Box::new(resolve_ty(inner, classes))),
        TypeRef::Array(inner, n) => Ty::Array(Box::new(resolve_ty(inner, classes)), *n),
        TypeRef::Fun {
            ret,
            params,
            variadic,
        } => Ty::Fun {
            ret: Box::new(resolve_ty(ret, classes)),
            params: params.iter().map(|p| resolve_ty(p, classes)).collect(),
            variadic: *variadic,
        },
    }
}

fn layout_class(decl: &ClassDecl, classes: &HashMap<String, ClassInfo>) -> ClassInfo {
    let mut members = Vec::new();
    let mut size = 0i64;
    if let Some(base) = &decl.base {
        if let Some(b) = classes.get(base) {
            members.extend(b.members.clone());
            size = b.size;
        }
    }
    let union_base = size;
    for m in &decl.members {
        let ty = resolve_ty(&m.ty, classes);
        let sz = ty.size().max(if matches!(ty, Ty::Ptr(_) | Ty::Fun { .. }) {
            8
        } else {
            ty.size()
        });
        let offset = if decl.union_ { union_base } else { size };
        if decl.union_ {
            size = size.max(union_base + sz);
        } else {
            size = offset + sz;
        }
        members.push(MemberInfo {
            name: m.name.clone(),
            ty,
            offset,
            size: sz,
        });
    }
    ClassInfo {
        name: decl.name.clone(),
        size,
        union_: decl.union_,
        members,
    }
}

fn packed_class(name: &str, fields: Vec<(&str, Ty)>) -> ClassInfo {
    let mut offset = 0;
    let mut members = Vec::with_capacity(fields.len());
    for (field_name, ty) in fields {
        let size = ty.size();
        members.push(MemberInfo {
            name: field_name.into(),
            ty,
            offset,
            size,
        });
        offset += size;
    }
    ClassInfo {
        name: name.into(),
        size: offset,
        union_: false,
        members,
    }
}
