//! Generic (helper-dispatched) binary ops for CLIF: arithmetic on values not
//! statically proven int (`Add`/`Sub`/`Mul`/`Div`/`Mod`) and generic
//! comparisons (`Eq`/`Neq`/`Lt`/`Gt`/`Lte`/`Gte`). These lower to a runtime
//! helper on boxed operands — they don't need the frame-aware machinery
//! (no home slots, no safepoint around them: a helper allocates at most, and
//! allocation never triggers a collection), so they work in ANY routed
//! function. Comparison results are unboxed to 0/1 so branches read them
//! directly; arithmetic results stay boxed (unboxed at an int use).
use super::alloc::AllocCtx;
use super::emit::{
    box_int, box_or_pass, call_helper, call_helper_void, meta_is_float, unbox_f64_coerce,
};
use super::kinds::K;
use crate::JitHelpers;
use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_core::OpCode;

/// General lowering context (not frame-specific). Helper addresses are passed
/// per-op, so this only carries the operand/dispatch essentials.
pub(crate) struct GenCtx<'a> {
    pub vars: &'a [Variable],
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub register_meta: &'a [varn_types::register_meta::RegisterMeta],
    pub jit_native_result_offset: usize,
    pub actx: Option<&'a AllocCtx<'a>>,
}

fn box_operand(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    r: usize,
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    let v = if let Some(actx) = g.actx {
        super::alloc::box_or_load_home(b, actx, state, r)
    } else {
        box_or_pass(b, g.vars, state, r)
    };
    if b.func.dfg.value_type(v) == types::I128 {
        b.ins().isplit(v)
    } else {
        (
            b.ins()
                .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64),
            v,
        )
    }
}

/// Define `dest` from a helper's boxed `VmValue` result, coercing when the
/// register is float-typed — its Variable is `F64`, so the raw bits would be
/// a Cranelift type error. Matches the kind `apply_kinds` assigns.
fn def_boxed(b: &mut FunctionBuilder, g: &GenCtx, dest: usize, res: cranelift_codegen::ir::Value) {
    if let Some(actx) = g.actx {
        super::alloc::def_result(b, actx, dest, res);
    } else if meta_is_float(g.register_meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(g.vars[dest], f);
    } else {
        let payload = if b.func.dfg.value_type(res) == types::I128 {
            let (_tag, payload) = b.ins().isplit(res);
            payload
        } else {
            res
        };
        b.def_var(g.vars[dest], payload);
    }
}

/// `dest, a, b` generic arithmetic via `helper(ctx, a, b) -> VmValue`.
/// Result is boxed bits.
pub(super) fn emit_binop(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    helper: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let a_r = (code[ip + 1] >> 8) as usize;
    let b_r = (code[ip + 1] & 0xFF) as usize;
    let (a_tag, a_payload) = box_operand(b, g, state, a_r);
    let (b_tag, b_payload) = box_operand(b, g, state, b_r);
    call_helper_void(
        b,
        g.cc,
        helper,
        &[g.exec_ctx, a_tag, a_payload, b_tag, b_payload],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        g.exec_ctx,
        g.jit_native_result_offset as i32,
    );
    def_boxed(b, g, dest, res);
}

/// `IsNull dest, src` — compare against the null VmValue bits (0/1 result).
pub(super) fn emit_is_null(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let is_null = match state[src] {
        K::Int | K::Float | K::Bool => b.ins().iconst(types::I64, 0),
        _ => {
            let (tag, _payload) = box_operand(b, g, state, src);
            let cmp = b
                .ins()
                .icmp_imm(IntCC::Equal, tag, varn_types::vm_value::KIND_NULL as i64);
            b.ins().uextend(types::I64, cmp)
        }
    };
    if let Some(actx) = g.actx {
        let boxed = super::emit::box_bool(b, is_null);
        super::alloc::def_result(b, actx, dest, boxed);
    } else {
        b.def_var(g.vars[dest], is_null);
    }
}

/// Dispatch a generic (helper-based) op. Returns `true` if `op` was one of
/// them and has been emitted, `false` otherwise (the caller then bails).
/// Keeps the whole family — arithmetic, comparisons, unary — in one place.
pub(super) fn try_emit(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    h: &JitHelpers,
    state: &[K],
    op: OpCode,
    code: &[u16],
    ip: usize,
) -> bool {
    // A raw 0/1 result cannot live in a float-typed register (its Variable is
    // `F64`), and a bool has no meaningful float representation — so a
    // comparison landing in one bails the whole function to the interpreter
    // rather than being coerced into nonsense.
    let bool_result = matches!(
        op,
        OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Gt
            | OpCode::Lte
            | OpCode::Gte
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::Instanceof
            | OpCode::In
            | OpCode::Not
            | OpCode::IsArray
            | OpCode::IsNull
    );
    let dest = (code[ip] >> 8) as usize;
    if bool_result && meta_is_float(g.register_meta, dest) {
        return false;
    }

    match op {
        OpCode::Add | OpCode::AddFloat => emit_binop(b, g, state, code, ip, h.add),
        OpCode::Sub | OpCode::SubFloat => emit_binop(b, g, state, code, ip, h.sub),
        OpCode::Mul | OpCode::MulFloat => emit_binop(b, g, state, code, ip, h.mul),
        OpCode::Div | OpCode::DivFloat | OpCode::DivInt => emit_binop(b, g, state, code, ip, h.div),
        OpCode::Mod | OpCode::ModFloat => emit_binop(b, g, state, code, ip, h.modulo),
        OpCode::Pow | OpCode::PowInt | OpCode::PowFloat => emit_binop(b, g, state, code, ip, h.pow),
        OpCode::Eq | OpCode::EqFloat => emit_compare(b, g, state, code, ip, h.eq, op),
        OpCode::Neq | OpCode::NeqFloat => emit_compare(b, g, state, code, ip, h.neq, op),
        OpCode::Lt | OpCode::LtFloat => emit_compare(b, g, state, code, ip, h.lt, op),
        OpCode::Gt | OpCode::GtFloat => emit_compare(b, g, state, code, ip, h.gt, op),
        OpCode::Lte | OpCode::LteFloat => emit_compare(b, g, state, code, ip, h.lte, op),
        OpCode::Gte | OpCode::GteFloat => emit_compare(b, g, state, code, ip, h.gte, op),
        OpCode::Instanceof => emit_compare(b, g, state, code, ip, h.instanceof, op),
        OpCode::In => emit_compare(b, g, state, code, ip, h.op_in, op),
        OpCode::BitAnd => emit_binop(b, g, state, code, ip, h.bit_and),
        OpCode::BitOr => emit_binop(b, g, state, code, ip, h.bit_or),
        OpCode::BitXor => emit_binop(b, g, state, code, ip, h.bit_xor),
        OpCode::Shl => emit_binop(b, g, state, code, ip, h.shl),
        OpCode::Shr => emit_binop(b, g, state, code, ip, h.shr),
        OpCode::Ushr => emit_binop(b, g, state, code, ip, h.ushr),
        OpCode::StrSlice => emit_binop(b, g, state, code, ip, h.str_slice),
        OpCode::StrLength => emit_str_length(b, g, state, code, ip, h.str_length),
        OpCode::ArrayPop => emit_unary(b, g, state, code, ip, h.array_pop),
        OpCode::Negate => emit_unary(b, g, state, code, ip, h.negate),
        OpCode::Typeof => emit_unary(b, g, state, code, ip, h.typeof_val),
        OpCode::ToString => emit_unary(b, g, state, code, ip, h.to_string),
        OpCode::Not => emit_unary_bool(b, g, state, code, ip, h.logical_not),
        OpCode::IsArray => emit_unary_bool(b, g, state, code, ip, h.is_array),
        OpCode::GetSymbol => emit_get_symbol(b, g, state, code, ip, h.get_symbol),
        OpCode::IsNull => emit_is_null(b, g, state, code, ip),
        _ => return false,
    }
    true
}

/// `dest, src` generic unary via `helper(ctx, v) -> VmValue` (Negate,
/// Typeof, ToString). Result is boxed bits.
pub(super) fn emit_unary(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    helper: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let (v_tag, v_payload) = box_operand(b, g, state, src);
    call_helper_void(b, g.cc, helper, &[g.exec_ctx, v_tag, v_payload]);
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        g.exec_ctx,
        g.jit_native_result_offset as i32,
    );
    def_boxed(b, g, dest, res);
}

/// `StrLength dest, src` with inline SSO fast-path.
pub(super) fn emit_str_length(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    helper: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let (tag, payload) = box_operand(b, g, state, src);

    let kind = b
        .ins()
        .band_imm(tag, varn_types::vm_value::KIND_MASK as i64);
    let is_sso = b
        .ins()
        .icmp_imm(IntCC::Equal, kind, varn_types::vm_value::KIND_SSO as i64);

    let fast = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);

    b.ins().brif(is_sso, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    let s = b.ins().ushr_imm(tag, 8);
    let sso_len = b.ins().band_imm(s, 0xFF);
    let boxed_len = box_int(b, sso_len);
    b.ins().jump(merge, &[boxed_len.into()]);

    b.switch_to_block(slow);
    call_helper_void(b, g.cc, helper, &[g.exec_ctx, tag, payload]);
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        g.exec_ctx,
        g.jit_native_result_offset as i32,
    );
    b.ins().jump(merge, &[res.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    def_boxed(b, g, dest, res);
}

/// `dest, src` unary producing a bool (0/1).
pub(super) fn emit_unary_bool(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    helper: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let (v_tag, v_payload) = box_operand(b, g, state, src);
    let cond = call_helper(b, g.cc, helper, &[g.exec_ctx, v_tag, v_payload]);
    if let Some(actx) = g.actx {
        let boxed = super::emit::box_bool(b, cond);
        super::alloc::def_result(b, actx, dest, boxed);
    } else {
        b.def_var(g.vars[dest], cond);
    }
}

/// `GetSymbol dest, obj, sym_idx` — well-known symbol property access;
/// `jit_get_symbol` reads the symbol from the current frame's closure.
pub(super) fn emit_get_symbol(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    get_symbol: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let obj_r = (code[ip + 1] >> 8) as usize;
    let sym_idx = code[ip + 2] as usize;
    let (obj_tag, obj_payload) = box_operand(b, g, state, obj_r);
    let sym = b.ins().iconst(types::I64, sym_idx as i64);
    call_helper_void(
        b,
        g.cc,
        get_symbol,
        &[g.exec_ctx, obj_tag, obj_payload, sym],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        g.exec_ctx,
        g.jit_native_result_offset as i32,
    );
    def_boxed(b, g, dest, res);
}

/// `dest, a, b` generic comparison via `helper(ctx, a, b) -> bool (0/1)`;
/// the result is 0/1 (`K::Bool`).
pub(super) fn emit_compare(
    b: &mut FunctionBuilder,
    g: &GenCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    helper: usize,
    op: OpCode,
) {
    let dest = (code[ip] >> 8) as usize;
    let a_r = (code[ip + 1] >> 8) as usize;
    let b_r = (code[ip + 1] & 0xFF) as usize;
    let (a_tag, a_payload) = box_operand(b, g, state, a_r);
    let (b_tag, b_payload) = box_operand(b, g, state, b_r);

    let is_eq = matches!(op, OpCode::Eq | OpCode::EqFloat | OpCode::EqInt);
    let is_neq = matches!(op, OpCode::Neq | OpCode::NeqFloat | OpCode::NeqInt);

    let cond = if is_eq || is_neq {
        let tag_eq = b.ins().icmp(IntCC::Equal, a_tag, b_tag);
        let pay_eq = b.ins().icmp(IntCC::Equal, a_payload, b_payload);
        let bits_eq = b.ins().band(tag_eq, pay_eq);

        let a_kind = b.ins().band_imm(a_tag, 0xFF);
        let not_float = b.ins().icmp_imm(IntCC::NotEqual, a_kind, 3);
        let same_non_float = b.ins().band(bits_eq, not_float);

        let a_is_sso = b.ins().icmp_imm(IntCC::Equal, a_kind, 5);
        let b_kind = b.ins().band_imm(b_tag, 0xFF);
        let b_is_sso = b.ins().icmp_imm(IntCC::Equal, b_kind, 5);
        let both_sso = b.ins().band(a_is_sso, b_is_sso);

        let can_inline = b.ins().bor(same_non_float, both_sso);

        let fast_blk = b.create_block();
        let slow_blk = b.create_block();
        let merge_blk = b.create_block();
        b.append_block_param(merge_blk, types::I64);

        b.ins().brif(can_inline, fast_blk, &[], slow_blk, &[]);

        b.switch_to_block(fast_blk);
        let fast_res = if is_eq {
            b.ins().uextend(types::I64, bits_eq)
        } else {
            let u = b.ins().uextend(types::I64, bits_eq);
            b.ins().bxor_imm(u, 1)
        };
        b.ins().jump(merge_blk, &[fast_res.into()]);

        b.switch_to_block(slow_blk);
        let slow_res = call_helper(
            b,
            g.cc,
            helper,
            &[g.exec_ctx, a_tag, a_payload, b_tag, b_payload],
        );
        b.ins().jump(merge_blk, &[slow_res.into()]);

        b.switch_to_block(merge_blk);
        b.block_params(merge_blk)[0]
    } else {
        call_helper(
            b,
            g.cc,
            helper,
            &[g.exec_ctx, a_tag, a_payload, b_tag, b_payload],
        )
    };

    if let Some(actx) = g.actx {
        let boxed = super::emit::box_bool(b, cond);
        super::alloc::def_result(b, actx, dest, boxed);
    } else {
        b.def_var(g.vars[dest], cond);
    }
}
