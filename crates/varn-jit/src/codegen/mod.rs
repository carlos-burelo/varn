pub(crate) mod arith;
pub(crate) mod arrays;
pub(crate) mod calls;
pub(crate) mod closures;
pub(crate) mod compare;
pub(crate) mod globals;
pub(crate) mod immediates;
pub(crate) mod indexing;
pub(crate) mod jumps;
pub(crate) mod modules;
pub(crate) mod properties;
pub(crate) mod strings;
pub(crate) mod unary;
pub(crate) mod misc;

pub(crate) use arith::emit_arith;
pub(crate) use arrays::emit_arrays;
pub(crate) use calls::emit_calls;
pub(crate) use closures::emit_closures;
pub(crate) use compare::emit_compare;
pub(crate) use globals::emit_globals;
pub(crate) use immediates::emit_immediates;
pub(crate) use indexing::emit_indexing;
pub(crate) use jumps::emit_jumps;
pub(crate) use modules::emit_modules;
pub(crate) use properties::emit_properties;
pub(crate) use strings::emit_strings;

use crate::assembler::Assembler;
use crate::regalloc::RegMap;
use varn_types::FunctionProto;

pub(crate) struct JumpPatch {
    pub patch_pos: usize,
    pub target_bytecode_ip: usize,
}

#[allow(dead_code)]
pub(crate) struct CodegenCtx<'a> {
    pub asm: Assembler,
    pub code: &'a [u16],
    pub ip: usize,
    pub regmap: RegMap,
    pub jump_patches: Vec<JumpPatch>,
    pub ip_to_asm_pos: Vec<usize>,
    pub proto: &'a FunctionProto,
    pub helpers: &'a crate::JitHelpers,
}
