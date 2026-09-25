//! Shared CLIF emission primitives for the lowering: value coercion
//! (box/unbox), the inline array-payload resolve chain and its loop cache,
//! and the runtime-helper call shim. Kept out of `lower.rs` so the
//! per-opcode walk there stays under the file-size governance limit; these
//! are the leaf builders that walk calls.
//!
//! Coercion here; the loop regions' hoisted caches in [`regions`], helper
//! calls and the payload walks in [`payload`].

use rustc_hash::FxHashMap as HashMap;

use cranelift_codegen::ir::{condcodes::IntCC, types, AbiParam, InstBuilder, MemFlags, Signature};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::SlotKind;

use super::alloc::AllocCtx;
use super::kinds::{is_boxed_kind, K};

mod payload;
mod regions;

pub(in crate::clif) use payload::*;
pub(in crate::clif) use regions::*;

// Tag tests, re-emitted inline. These apply to a value's TAG WORD; the
// payload word carries the datum and is never masked. Under the NaN-box
// these were 64-bit constants that had to come from a constant pool — as
// small immediates they now fold into the compare.
pub(super) const KIND_MASK: i64 = varn_types::vm_value::KIND_MASK as i64;
pub(super) const HEAP_KIND: i64 = varn_types::vm_value::KIND_HEAP as i64;

/// `site` is the buffer offset of the rel32 field; `target` the buffer
/// offset the call must reach.
pub(super) fn patch_rel32(buf: &mut [u8], site: usize, target: usize) {
    let disp = target as i64 - (site as i64 + 4);
    let disp = i32::try_from(disp).expect("clif: rel32 out of range");
    buf[site..site + 4].copy_from_slice(&disp.to_le_bytes());
}

pub(super) fn meta_is_int(meta: &[varn_types::register_meta::RegisterMeta], r: usize) -> bool {
    meta.get(r).is_some_and(|m| m.kind == SlotKind::Int)
}

/// Whether register `r`'s physical class is heap-classed (`Ref` or `Dyn`),
/// i.e. its variable is an I128 tag+payload pair rather than a scalar word.
pub(super) fn dest_is_ref(meta: &[varn_types::register_meta::RegisterMeta], r: usize) -> bool {
    matches!(
        meta.get(r)
            .map(|m| varn_types::register_meta::SlotClass::of_kind(m.kind)),
        Some(varn_types::register_meta::SlotClass::Ref | varn_types::register_meta::SlotClass::Dyn)
    )
}

pub(super) fn def_const(b: &mut FunctionBuilder, vars: &[Variable], reg: usize, v: i64) {
    let c = b.ins().iconst(types::I64, v);
    b.def_var(vars[reg], c);
}

pub(super) fn def_const_int(
    b: &mut FunctionBuilder,
    actx: Option<&AllocCtx>,
    meta: &[varn_types::register_meta::RegisterMeta],
    vars: &[Variable],
    reg: usize,
    v: i64,
) {
    if dest_is_ref(meta, reg) {
        let c = b.ins().iconst(types::I64, v);
        let boxed = box_int(b, c);
        b.def_var(vars[reg], boxed);
        if let Some(actx) = actx {
            super::alloc::store_boxed_home(b, actx, reg, boxed);
        }
    } else if meta_is_float(meta, reg) {
        let f = b.ins().f64const(v as f64);
        b.def_var(vars[reg], f);
        if let Some(actx) = actx {
            let boxed = box_f64(b, f);
            super::alloc::store_boxed_home(b, actx, reg, boxed);
        }
    } else {
        let c = b.ins().iconst(types::I64, v);
        b.def_var(vars[reg], c);
        if let Some(actx) = actx {
            let boxed = box_int(b, c);
            super::alloc::store_boxed_home(b, actx, reg, boxed);
        }
    }
}

pub(super) fn def_const_bool(
    b: &mut FunctionBuilder,
    actx: Option<&AllocCtx>,
    meta: &[varn_types::register_meta::RegisterMeta],
    vars: &[Variable],
    reg: usize,
    v: bool,
) {
    let c = b.ins().iconst(types::I64, if v { 1 } else { 0 });
    let boxed = box_bool(b, c);
    if dest_is_ref(meta, reg) {
        b.def_var(vars[reg], boxed);
    } else {
        b.def_var(vars[reg], c);
    }
    if let Some(actx) = actx {
        super::alloc::store_boxed_home(b, actx, reg, boxed);
    }
}

/// Define a register from a BOXED `VmValue` (`I128`; a bare `I64` is treated as
/// an int payload by the same rule `def_result` uses), honouring the
/// destination's physical class. This is `alloc::def_result` for a lowering
/// that has no frame (`actx == None`, i.e. a leaf): a heap-classed (`Dyn`/`Ref`)
/// destination keeps the whole pair instead of a raw payload being written into
/// an `I128` Variable (a Cranelift type error, and the C1 value-flow bug).
pub(super) fn def_boxed_leaf(
    b: &mut FunctionBuilder,
    meta: &[varn_types::register_meta::RegisterMeta],
    vars: &[Variable],
    dest: usize,
    res: cranelift_codegen::ir::Value,
) {
    if dest_is_ref(meta, dest) {
        let pair = if b.func.dfg.value_type(res) == types::I128 {
            res
        } else {
            let tag = b
                .ins()
                .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
            b.ins().iconcat(tag, res)
        };
        b.def_var(vars[dest], pair);
    } else if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(vars[dest], f);
    } else {
        let payload = if b.func.dfg.value_type(res) == types::I128 {
            let (_tag, payload) = b.ins().isplit(res);
            payload
        } else {
            res
        };
        b.def_var(vars[dest], payload);
    }
}

/// Define a register from a RAW integer result, boxing it for a heap-classed
/// destination. The single place an int-producing op writes its destination,
/// so every producer agrees with `use_int`/`JumpIfFalse` about the pair.
pub(super) fn def_int_result(
    b: &mut FunctionBuilder,
    actx: Option<&AllocCtx>,
    meta: &[varn_types::register_meta::RegisterMeta],
    vars: &[Variable],
    dest: usize,
    raw: cranelift_codegen::ir::Value,
) {
    let boxed = box_int(b, raw);
    match actx {
        Some(actx) => super::alloc::def_result(b, actx, dest, boxed),
        None => def_boxed_leaf(b, meta, vars, dest, boxed),
    }
}

/// Define a register from a raw 0/1 bool result, boxing it for a heap-classed
/// destination (bool registers are `SlotClass::Dyn`, hence an `I128` pair).
pub(super) fn def_bool_result(
    b: &mut FunctionBuilder,
    actx: Option<&AllocCtx>,
    meta: &[varn_types::register_meta::RegisterMeta],
    vars: &[Variable],
    dest: usize,
    raw: cranelift_codegen::ir::Value,
) {
    let boxed = box_bool(b, raw);
    match actx {
        Some(actx) => super::alloc::def_result(b, actx, dest, boxed),
        None => def_boxed_leaf(b, meta, vars, dest, boxed),
    }
}

/// Read a register as an unboxed int. `Int` vars are already raw; a boxed
/// `VmValue`'s payload word IS the raw i64, so it's read unchanged too.
pub(super) fn use_int(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    r: usize,
) -> Result<cranelift_codegen::ir::Value, String> {
    match state[r] {
        K::Int => Ok(b.use_var(vars[r])),
        k if is_boxed_kind(k) => {
            let v = b.use_var(vars[r]);
            // A paired (heap-classed) variable: the payload word is the int.
            if b.func.dfg.value_type(v) == types::I128 {
                Ok(b.ins().isplit(v).1)
            } else {
                Ok(v)
            }
        }
        k => Err(format!("clif: int use of {k:?} register")),
    }
}

/// Read a register as boxed VmValue bits (heap receivers, call args).
pub(super) fn use_boxed(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    r: usize,
) -> Result<cranelift_codegen::ir::Value, String> {
    if is_boxed_kind(state[r]) {
        Ok(b.use_var(vars[r]))
    } else {
        Err(format!("clif: boxed use of {:?} register r{r}", state[r]))
    }
}

/// The value a `Return src` yields, coerced to the raw function's return
/// convention: an int return stays an unboxed i64 payload (the wrapper and
/// the clif→clif fast call re-tag it); every other return kind yields boxed
/// VmValue bits that the wrapper passes through. A reachable `null` (a
/// constructor's implicit `return null`) short-circuits to the null bits.
pub(super) fn emit_return_value(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    return_kind: SlotKind,
    src: usize,
) -> Result<cranelift_codegen::ir::Value, String> {
    if state[src] == K::Poison {
        return Ok(match return_kind {
            SlotKind::Float => b.ins().f64const(0.0),
            SlotKind::Int | SlotKind::Bool => b.ins().iconst(types::I64, 0),
            _ => box_null(b),
        });
    }
    Ok(match return_kind {
        SlotKind::Int => match state[src] {
            K::Int => b.use_var(vars[src]),
            K::Boxed => use_int(b, vars, state, src)?,
            _ => {
                let raw = b.use_var(vars[src]);
                if b.func.dfg.value_type(raw) == types::F64 {
                    let iv = b.ins().fcvt_to_sint(types::I64, raw);
                    unbox_int(b, iv)
                } else {
                    let v = box_or_pass(b, vars, state, src);
                    unbox_int(b, v)
                }
            }
        },
        SlotKind::Float => match state[src] {
            K::Float => {
                let raw = b.use_var(vars[src]);
                if b.func.dfg.value_type(raw) == types::F64 {
                    raw
                } else {
                    b.ins().bitcast(types::F64, MemFlags::new(), raw)
                }
            }
            K::Int => {
                let iv = b.use_var(vars[src]);
                if b.func.dfg.value_type(iv) == types::F64 {
                    iv
                } else {
                    b.ins().fcvt_from_sint(types::F64, iv)
                }
            }
            _ => {
                let v = box_or_pass(b, vars, state, src);
                unbox_f64_coerce(b, v)
            }
        },
        SlotKind::Bool => match state[src] {
            K::Bool => b.use_var(vars[src]),
            _ => {
                let v = box_or_pass(b, vars, state, src);
                unbox_bool(b, v)
            }
        },
        SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => box_or_pass(b, vars, state, src),
    })
}

/// Re-tag an unboxed int as a VmValue.
// Parameters kept, unused: they document the signature the migrated
// version has to satisfy.
#[allow(unused_variables)]
pub(super) fn box_int(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_INT as i64);
    b.ins().iconcat(tag, v)
}

/// Box an unboxed 0/1 bool as a VmValue: `false` = 0x7FFA…, `true` = 0x7FFB…
/// (`0x7FFA_0000_0000_0000 | (v << 48)`, valid because v ∈ {0,1}).
// Parameters kept, unused: they document the signature the migrated
// version has to satisfy.
pub(super) fn box_bool(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_BOOL as i64);
    let payload = if b.func.dfg.value_type(v) == types::I64 {
        v
    } else {
        b.ins().uextend(types::I64, v)
    };
    b.ins().iconcat(tag, payload)
}

/// Unbox a boxed bool VmValue (TAG_TRUE=0x7FFB…, TAG_FALSE=0x7FFA…) to 0/1:
/// the two tags differ only in bit 48, so `(v >> 48) & 1`.
// Parameters kept, unused: they document the signature the migrated
// version has to satisfy.
pub(super) fn unbox_bool(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    if b.func.dfg.value_type(v) == types::I128 {
        let (_tag, payload) = b.ins().isplit(v);
        payload
    } else {
        v
    }
}

/// Box an unboxed `f64` as a VmValue, replicating `VmValue::from_f64`: a
/// float is stored as its raw bits EXCEPT a quiet-NaN-range result
/// (`bits & QNAN == QNAN`) canonicalizes to `null`. This keeps a native float
/// result byte-identical to the interpreter, which routes every float op
/// through `from_f64`.
// Parameters kept, unused: they document the signature the migrated
// version has to satisfy.
pub(super) fn box_f64(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_FLOAT as i64);
    let payload = b.ins().bitcast(types::I64, MemFlags::new(), v);
    b.ins().iconcat(tag, payload)
}

/// Unbox a VmValue a float slot may hold as EITHER float bits OR an int
/// VmValue — the latter arises when a widening int argument is passed to a
/// float parameter (`takesFloat(5)`), where the caller boxes the int and the
/// callee must coerce, exactly as the interpreter's `to_f64_val` does. An
/// int-tagged value `fcvt`s its raw i64 payload; anything else is
/// reinterpreted as its f64 bits.
// Parameters kept, unused: they document the signature the migrated
// version has to satisfy.
pub(super) fn unbox_f64_coerce(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let ty = b.func.dfg.value_type(v);
    if ty == types::F64 {
        v
    } else if ty == types::I128 {
        let (tag, payload) = b.ins().isplit(v);
        let is_float = b.ins().icmp_imm(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            tag,
            varn_types::vm_value::KIND_FLOAT as i64,
        );
        let f_direct = b.ins().bitcast(types::F64, MemFlags::new(), payload);
        let f_from_int = b.ins().fcvt_from_sint(types::F64, payload);
        b.ins().select(is_float, f_direct, f_from_int)
    } else if ty == types::I64 {
        b.ins().fcvt_from_sint(types::F64, v)
    } else {
        let ext = b.ins().sextend(types::I64, v);
        b.ins().fcvt_from_sint(types::F64, ext)
    }
}

/// Read a register as a raw `f64`: a `Float` var is already `F64`; an `Int`
/// var coerces via `fcvt_from_sint` (the interpreter's `to_f64_val` does the
/// same int→float widening in a mixed float op).
pub(super) fn use_f64(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    r: usize,
) -> Result<cranelift_codegen::ir::Value, String> {
    let st = state.get(r).copied().unwrap_or(K::Unset);
    let Some(&var) = vars.get(r) else {
        return Err(format!("clif: invalid register index {r}"));
    };
    match st {
        K::Float => {
            let raw = b.use_var(var);
            if b.func.dfg.value_type(raw) == types::F64 {
                Ok(raw)
            } else {
                Ok(b.ins().bitcast(types::F64, MemFlags::new(), raw))
            }
        }
        K::Int => {
            let iv = b.use_var(var);
            if b.func.dfg.value_type(iv) == types::F64 {
                Ok(iv)
            } else {
                Ok(b.ins().fcvt_from_sint(types::F64, iv))
            }
        }
        k => Err(format!("clif: f64 use of {k:?} register")),
    }
}

/// Whether register `r` is float-typed (its Variable is declared `F64`).
pub(super) fn meta_is_float(meta: &[varn_types::register_meta::RegisterMeta], r: usize) -> bool {
    meta.get(r).is_some_and(|m| m.kind == SlotKind::Float)
}

/// Read a register as boxed VmValue bits regardless of its representation:
/// int → `box_int`, bool → `box_bool`, float → `box_f64`, already-boxed (or
/// any other tracked kind) → the raw bits. Callers pass registers whose
/// lattice kind the flow has already proven to hold a real value.
pub(super) fn box_null(b: &mut FunctionBuilder) -> cranelift_codegen::ir::Value {
    let tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_NULL as i64);
    let zero = b.ins().iconst(types::I64, 0);
    b.ins().iconcat(tag, zero)
}

pub(super) fn box_or_pass(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    r: usize,
) -> cranelift_codegen::ir::Value {
    let Some(&var) = vars.get(r) else {
        return box_null(b);
    };
    let raw = b.use_var(var);
    if b.func.dfg.value_type(raw) == types::I128 {
        // A pair variable (a `Ref` register) already holds a boxed VmValue.
        raw
    } else if b.func.dfg.value_type(raw) == types::F64 {
        box_f64(b, raw)
    } else {
        match state.get(r).copied().unwrap_or(K::Unset) {
            K::Int => box_int(b, raw),
            K::Bool => box_bool(b, raw),
            K::Float => {
                let f = b.ins().bitcast(types::F64, MemFlags::new(), raw);
                box_f64(b, f)
            }
            _ => {
                let tag = b
                    .ins()
                    .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
                b.ins().iconcat(tag, raw)
            }
        }
    }
}

pub(super) fn state_meta_int(meta: &[varn_types::register_meta::RegisterMeta], r: usize) -> bool {
    meta.get(r).is_some_and(|m| m.kind == SlotKind::Int)
}
