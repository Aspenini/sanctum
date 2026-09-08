//! TempleOS-shaped intermediate codes (`IC_*` from CompilerA.HH).
//! The Cranelift backend currently lowers AST directly; this enum is the
//! stable IR we'll share once optimizer passes exist.

#![allow(non_camel_case_types)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Ic {
    END = 0x00,
    NOP1 = 0x01,
    IMM_I64 = 0x0A,
    IMM_F64 = 0x0B,
    STR_CONST = 0x0C,
    FS = 0x15,
    GS = 0x17,
    MOV = 0x1A,
    TO_I64 = 0x1B,
    TO_F64 = 0x1C,
    TO_BOOL = 0x1D,
    HOLYC_TYPECAST = 0x1F,
    ADDR = 0x20,
    COM = 0x21,
    NOT = 0x22,
    UNARY_MINUS = 0x23,
    DEREF = 0x24,
    SHL = 0x2B,
    SHR = 0x2C,
    POWER = 0x2F,
    MUL = 0x30,
    DIV = 0x31,
    MOD = 0x32,
    AND = 0x33,
    OR = 0x34,
    XOR = 0x35,
    ADD = 0x36,
    SUB = 0x37,
    EQU_EQU = 0x3A,
    NOT_EQU = 0x3B,
    LESS = 0x3C,
    GREATER_EQU = 0x3D,
    GREATER = 0x3E,
    LESS_EQU = 0x3F,
    AND_AND = 0x41,
    OR_OR = 0x42,
    XOR_XOR = 0x43,
    ASSIGN = 0x44,
    JMP = 0x51,
    CALL = 0x5C,
    RET = 0x5B,
    BT = 0x77,
    QUE_INIT = 0x80,
    STRLEN = 0x84,
}
