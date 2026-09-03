//! Safepoint tracking and root flushes/reloads for CLIF allocation lowering.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use std::cell::{Cell, RefCell};
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::register_meta::RegisterMeta;

use super::super::emit::{call_helper_void, meta_is_float, unbox_bool, unbox_f64_coerce, wrap_i48};
use super::super::kinds::K;
use super::super::liveness::Liveness;
use crate::JitHelpers;

/// How precisely an allocation scan reads `OpCode::Intrinsic`.
///
/// The two callers ask different questions of the same opcode list. A whole
/// FUNCTION is scanned to decide whether it needs a frame, safepoints and
/// root flushes, and there the cost of a false `true` is a slower prologue
/// while the cost of a false `false` is a missed root — so it stays
/// conservative. A loop REGION is scanned to decide whether a resolved
/// pointer may be hoisted out of it, and there an `Intrinsic` that provably
/// allocates nothing is the difference between hoisting and not.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntrinsicScan {
    /// Every `Intrinsic` counts as allocating.
    Conservative,
    /// An `Intrinsic` counts only when
    /// [`varn_core::intrinsic_ops::intrinsic_allocates`] says its wire byte can
    /// allocate.
    ByWireByte,
}

/// Conservative whole-function scan; see [`IntrinsicScan`].
pub(crate) fn has_alloc(
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
) -> Result<bool, String> {
    has_alloc_scan(code, pool, IntrinsicScan::Conservative)
}

pub(crate) fn has_alloc_scan(
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
    scan: IntrinsicScan,
) -> Result<bool, String> {
    let mut ip = 0usize;
    while ip < code.len() {
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        // The wire byte lives in the high half of the operand word, the same
        // place `strings::emit_str_intrinsic_native` reads it from.
        if scan == IntrinsicScan::ByWireByte
            && OpCode::from_u8(code[ip] as u8) == Some(OpCode::Intrinsic)
            && !varn_core::intrinsic_ops::intrinsic_allocates((code[ip + 1] >> 8) as u8)
        {
            ip += info.len;
            continue;
        }
        if matches!(
            OpCode::from_u8(code[ip] as u8),
            Some(
                OpCode::BuildArray
                    | OpCode::BuildTuple
                    | OpCode::BuildObject
                    | OpCode::BuildObjectWithShape
                    | OpCode::BuildRecord
                    | OpCode::ArrayPush
                    | OpCode::ArrayExtend
                    | OpCode::MakeEnumVariant
                    | OpCode::StrConcat
                    | OpCode::BuildStr
                    | OpCode::CallNativeOp
                    | OpCode::Add
                    | OpCode::Sub
                    | OpCode::Mul
                    | OpCode::Div
                    | OpCode::DivInt
                    | OpCode::Mod
                    | OpCode::Pow
                    | OpCode::BitAnd
                    | OpCode::BitOr
                    | OpCode::BitXor
                    | OpCode::Shl
                    | OpCode::Shr
                    | OpCode::Ushr
                    | OpCode::GetProperty
                    | OpCode::SetProperty
                    | OpCode::Call
                    | OpCode::CallMethod
                    | OpCode::InvokeVirtual
                    | OpCode::ToString
                    | OpCode::Typeof
                    | OpCode::Negate
                    | OpCode::GetSymbol
                    | OpCode::StrSlice
                    | OpCode::Intrinsic
                    | OpCode::MakeClosure
                    | OpCode::MakeClass
                    | OpCode::LoadUpvalue
                    | OpCode::StoreUpvalue
                    | OpCode::CloseUpvalue
                    | OpCode::LoadStaticFn
                    | OpCode::LoadModule
                    | OpCode::LoadModuleSlot
                    | OpCode::StoreModuleSlot
                    | OpCode::GetSuper
                    | OpCode::DeclareField
                    | OpCode::Method
                    | OpCode::DefineStatic
                    | OpCode::DefineGetter
                    | OpCode::DefineSetter
                    | OpCode::DefineStaticGetter
                    | OpCode::DefineStaticSetter
                    | OpCode::Inherit
                    | OpCode::BindMethod
                    | OpCode::Try
                    | OpCode::Throw
                    | OpCode::PopTry
                    | OpCode::Yield
                    | OpCode::Await
                    | OpCode::Spawn
                    | OpCode::ObjectRest
                    | OpCode::ObjectKeys
                    | OpCode::ObjectMerge
                    | OpCode::CallSpread
                    | OpCode::WrapSpread
                    | OpCode::GetIndex
                    | OpCode::SetIndex
            )
        ) {
            return Ok(true);
        }
        ip += info.len;
    }
    Ok(false)
}

pub(crate) fn has_try(code: &[u16], pool: &[varn_types::chunk::PoolEntry]) -> Result<bool, String> {
    let mut ip = 0usize;
    while ip < code.len() {
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        if OpCode::from_u8(code[ip] as u8) == Some(OpCode::Try) {
            return Ok(true);
        }
        ip += info.len;
    }
    Ok(false)
}

pub(crate) type SafepointRecord = (usize, Vec<usize>, Vec<usize>);

pub(crate) struct AllocCtx<'a> {
    pub vars: &'a [Variable],
    pub helpers: &'a JitHelpers,
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub base: cranelift_codegen::ir::Value,
    pub closure: cranelift_codegen::ir::Value,
    pub nregs: usize,
    pub register_meta: &'a [RegisterMeta],
    pub live: &'a Liveness,
    pub narrow_roots: bool,
    pub cur_ip: Cell<usize>,
    pub safepoints: Option<RefCell<Vec<SafepointRecord>>>,
}

pub(crate) fn frame_base_addr(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
) -> cranelift_codegen::ir::Value {
    let sp = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.stack_data_offset as i32,
    );
    let base_bytes = b.ins().ishl_imm(actx.base, 4);
    b.ins().iadd(sp, base_bytes)
}

pub(crate) fn load_receiver(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
) -> cranelift_codegen::ir::Value {
    load_home(b, actx, 0)
}

pub(crate) fn load_home(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    r: usize,
) -> cranelift_codegen::ir::Value {
    let fb = frame_base_addr(b, actx);
    b.ins()
        .load(types::I128, MemFlags::trusted(), fb, (r * 16) as i32)
}

pub(crate) fn box_or_load_home(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    r: usize,
) -> cranelift_codegen::ir::Value {
    let Some(&var) = actx.vars.get(r) else {
        return super::super::emit::box_null(b);
    };
    let raw = b.use_var(var);
    if b.func.dfg.value_type(raw) == types::F64 {
        super::super::emit::box_f64(b, raw)
    } else {
        match state.get(r).copied().unwrap_or(K::Unset) {
            K::Int => super::super::emit::box_int(b, raw),
            K::Bool => super::super::emit::box_bool(b, raw),
            _ => load_home(b, actx, r),
        }
    }
}

pub(crate) fn store_home(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    fb: cranelift_codegen::ir::Value,
    reg: usize,
) {
    let Some(&var) = actx.vars.get(reg) else {
        let null_val = super::super::emit::box_null(b);
        b.ins().store(MemFlags::trusted(), null_val, fb, (reg * 16) as i32);
        return;
    };
    let raw = b.use_var(var);
    if b.func.dfg.value_type(raw) == types::F64 {
        let v = super::super::emit::box_f64(b, raw);
        b.ins().store(MemFlags::trusted(), v, fb, (reg * 16) as i32);
    } else {
        match state.get(reg).copied().unwrap_or(K::Unset) {
            K::Int => {
                let v = super::super::emit::box_int(b, raw);
                b.ins().store(MemFlags::trusted(), v, fb, (reg * 16) as i32);
            }
            K::Bool => {
                let v = super::super::emit::box_bool(b, raw);
                b.ins().store(MemFlags::trusted(), v, fb, (reg * 16) as i32);
            }
            _ => {
                // The slot on `ctx.stack` already holds the boxed value (stored on
                // def_result, Move, or entry). Redundant load + store to same slot is skipped.
            }
        }
    }
}

pub(crate) fn def_result(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    dest: usize,
    res: cranelift_codegen::ir::Value,
) {
    let fb = frame_base_addr(b, actx);
    if meta_is_float(actx.register_meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        let payload = if b.func.dfg.value_type(res) == types::I128 {
            let (_tag, payload) = b.ins().isplit(res);
            payload
        } else {
            res
        };
        b.def_var(actx.vars[dest], payload);
    }
    b.ins()
        .store(MemFlags::trusted(), res, fb, (dest * 16) as i32);
}

pub(crate) fn emit_backedge_safepoint(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    payload_caches: &[Variable],
) {
    let h = actx.helpers;
    let rcbox = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        actx.exec_ctx,
        h.heap_field_offset as i32,
    );
    let len = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        rcbox,
        h.nursery_len_offset as i32,
    );
    let over = b.ins().icmp_imm(
        IntCC::UnsignedGreaterThanOrEqual,
        len,
        h.nursery_threshold as i64,
    );
    let slow = b.create_block();
    let cont = b.create_block();
    b.ins().brif(over, slow, &[], cont, &[]);

    b.switch_to_block(slow);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(b, actx.cc, h.gc_safepoint, &[actx.exec_ctx]);
    reload_boxed(b, actx, state, &regs);
    let invalid = b.ins().iconst(types::I64, 0);
    for &cv in payload_caches {
        b.def_var(cv, invalid);
    }
    b.ins().jump(cont, &[]);
    b.switch_to_block(cont);
}

pub(crate) fn live_boxed(actx: &AllocCtx, state: &[K]) -> Vec<usize> {
    let ip = actx.cur_ip.get();
    let live_nonfloat = (0..actx.nregs)
        .filter(|&r| !meta_is_float(actx.register_meta, r))
        .filter(|&r| actx.live.is_live_after(ip, r));
    let rooted = |r: usize| !actx.narrow_roots || state.get(r).copied().is_none_or(is_root_kind);
    let regs: Vec<usize> = live_nonfloat.clone().filter(|&r| rooted(r)).collect();
    if let Some(rec) = &actx.safepoints {
        let unboxed: Vec<usize> = live_nonfloat.filter(|&r| !rooted(r)).collect();
        rec.borrow_mut().push((ip, regs.clone(), unboxed));
    }
    regs
}

fn is_root_kind(k: K) -> bool {
    !matches!(k, K::Int | K::Bool)
}

pub(crate) fn flush_boxed(b: &mut FunctionBuilder, actx: &AllocCtx, state: &[K], regs: &[usize]) {
    let fb = frame_base_addr(b, actx);
    for &r in regs {
        store_home(b, actx, state, fb, r);
    }
}

pub(crate) fn reload_boxed(b: &mut FunctionBuilder, actx: &AllocCtx, state: &[K], regs: &[usize]) {
    let fb = frame_base_addr(b, actx);
    for &r in regs {
        let v = b
            .ins()
            .load(types::I128, MemFlags::trusted(), fb, (r * 16) as i32);
        if meta_is_float(actx.register_meta, r) {
            let f = unbox_f64_coerce(b, v);
            b.def_var(actx.vars[r], f);
        } else {
            let restored = match state[r] {
                K::Int => wrap_i48(b, v),
                K::Bool => unbox_bool(b, v),
                _ => {
                    let (_tag, payload) = b.ins().isplit(v);
                    payload
                }
            };
            b.def_var(actx.vars[r], restored);
        }
    }
}
