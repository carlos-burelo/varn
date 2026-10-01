//! Full-family extension of the SSA lowering: every portable op outside the
//! scalar/heap core.
//!
//! Each arm mirrors the interpreter's own helper (same `jit_*` entry the
//! bytecode lowering calls), so there is one runtime per fact (Ley 6). Ops
//! that can suspend (`Await`/`Yield`/`LoadModule`) spill every defined scalar
//! to its home first — the interpreter resumes from homes — then trap, since
//! the suspend helper never returns to compiled code.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::{SsaObjectSpreadPart, SsaOp, SsaSpread};

use super::heap::{boxed_value, exec_ctx};
use super::store::{def_heap, use_heap, Out};
use super::{load_value, Ctx};

use super::super::emit::call_helper_void;

/// Try to emit `op`. `Ok(None)` means it is not an extra op and the caller
/// (scalar/heapvalue) must handle it. `Ok(Some(out))` carries the result;
/// `Ok(Some(None))` is an effect op with no result.
pub(super) fn try_emit(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    op: &SsaOp,
    dest: Option<u32>,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let frame = || {
        ctx.frame
            .as_ref()
            .ok_or("from_ssa: extra op without a frame")
    };
    let native = |b: &mut FunctionBuilder| {
        b.ins().load(
            types::I128,
            MemFlags::trusted(),
            ectx,
            h.jit_native_result_offset as i32,
        )
    };
    match op {
        SsaOp::LoadGlobal(name) => {
            let f = frame()?;
            let ni = super::props::str_idx(ctx, name)? as i64;
            let niv = b.ins().iconst(types::I64, ni);
            call_helper_void(b, ctx.cc, h.load_global_by_name, &[ectx, f.closure, niv]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::StoreGlobal { name, value } => {
            let f = frame()?;
            let boxed = boxed_value(b, ctx, values, *value)?;
            let (t, p) = b.ins().isplit(boxed);
            let niv = b
                .ins()
                .iconst(types::I64, super::props::str_idx(ctx, name)? as i64);
            call_helper_void(
                b,
                ctx.cc,
                h.store_global_by_name,
                &[ectx, f.closure, niv, t, p],
            );
            Ok(Some(None))
        }
        SsaOp::BuildTuple { elements } => {
            let vals: Vec<Value> = elements
                .iter()
                .map(|v| boxed_value(b, ctx, values, *v))
                .collect::<Result<_, _>>()?;
            let r = window_result(b, ctx, h.build_array_window, &vals);
            Ok(Some(Some(Out::Boxed(r))))
        }
        SsaOp::BuildArraySpread { elements } => {
            frame()?;
            let r = emit_array_spread(b, ctx, values, elements)?;
            Ok(Some(Some(Out::Boxed(r))))
        }
        SsaOp::BuildObjectSpread { parts, cs_base } => {
            frame()?;
            let r = emit_object_spread(b, ctx, values, parts, *cs_base)?;
            Ok(Some(Some(Out::Boxed(r))))
        }
        SsaOp::ObjectMerge { target, source } => {
            let a = boxed_value(b, ctx, values, *target)?;
            let c = boxed_value(b, ctx, values, *source)?;
            let (at, ap) = b.ins().isplit(a);
            let (ct, cp) = b.ins().isplit(c);
            call_helper_void(b, ctx.cc, h.object_merge, &[ectx, at, ap, ct, cp]);
            let r = native(b);
            def_heap(b, ctx, ctx.ssa.reg(*target), r)?;
            Ok(Some(None))
        }
        SsaOp::ObjectRest { object, skip_keys } => {
            let o = boxed_value(b, ctx, values, *object)?;
            let (ot, op_) = b.ins().isplit(o);
            let mut keys = Vec::with_capacity(skip_keys.len());
            for k in skip_keys {
                keys.push(super::pool::literal(
                    b,
                    ctx,
                    "string",
                    |l| matches!(l, varn_types::Literal::Str(t) if t.as_ref() == k.as_ref()),
                )?);
            }
            let (addr, n) = stage(b, ctx, &keys);
            call_helper_void(b, ctx.cc, h.object_rest_window, &[ectx, ot, op_, addr, n]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::GetPropertyMaybe { object, name } => {
            let o = boxed_value(b, ctx, values, *object)?;
            let (ot, op_) = b.ins().isplit(o);
            let niv = b
                .ins()
                .iconst(types::I64, super::props::str_idx(ctx, name)? as i64);
            call_helper_void(b, ctx.cc, h.get_property_maybe, &[ectx, ot, op_, niv]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::AssertNotNull { operand } => {
            let v = boxed_value(b, ctx, values, *operand)?;
            let (t, p) = b.ins().isplit(v);
            call_helper_void(b, ctx.cc, h.assert_not_null, &[ectx, t, p]);
            Ok(Some(None))
        }
        SsaOp::BindMethod { object, name } => {
            let o = boxed_value(b, ctx, values, *object)?;
            let (ot, op_) = b.ins().isplit(o);
            let niv = b
                .ins()
                .iconst(types::I64, super::props::str_idx(ctx, name)? as i64);
            call_helper_void(b, ctx.cc, h.bind_method, &[ectx, ot, op_, niv]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::ArrayExtend { array, source } => {
            let a = boxed_value(b, ctx, values, *array)?;
            let s = boxed_value(b, ctx, values, *source)?;
            let (at, ap) = b.ins().isplit(a);
            let (st, sp) = b.ins().isplit(s);
            call_helper_void(b, ctx.cc, h.array_extend, &[ectx, at, ap, st, sp]);
            Ok(Some(None))
        }
        SsaOp::WrapSpread { operand } => {
            let v = boxed_value(b, ctx, values, *operand)?;
            let (t, p) = b.ins().isplit(v);
            call_helper_void(b, ctx.cc, h.wrap_spread, &[ectx, t, p]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::Range {
            start,
            end,
            inclusive,
        } => {
            let a = boxed_value(b, ctx, values, *start)?;
            let c = boxed_value(b, ctx, values, *end)?;
            let (at, ap) = b.ins().isplit(a);
            let (ct, cp) = b.ins().isplit(c);
            let f = b.ins().iconst(types::I64, i64::from(*inclusive));
            call_helper_void(b, ctx.cc, h.range, &[ectx, at, ap, ct, cp, f]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::GetSymbol { object, is_async } => {
            let o = boxed_value(b, ctx, values, *object)?;
            let (ot, op_) = b.ins().isplit(o);
            let want = if *is_async {
                varn_types::value::RuntimeSymbol::AsyncIterator
            } else {
                varn_types::value::RuntimeSymbol::Iterator
            };
            let idx = ctx
                .proto
                .chunk
                .constants
                .iter()
                .position(|e| {
                    matches!(e, varn_types::PoolEntry::Literal(varn_types::Literal::Symbol(s)) if *s == want)
                })
                .ok_or("from_ssa: iterator symbol not in pool")? as i64;
            let siv = b.ins().iconst(types::I64, idx);
            call_helper_void(b, ctx.cc, h.get_symbol, &[ectx, ot, op_, siv]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::IterCall { callee, recv } => {
            frame()?;
            let c = load_value(b, ctx, values, *callee)?;
            let (ct, cp) = b.ins().isplit(c);
            let w = super::call::boxed_window(b, ctx, values, c, &[*recv])?;
            let r = super::call::emit_invoke(b, ctx, w, (ct, cp), 2);
            Ok(Some(Some(Out::Boxed(r))))
        }
        SsaOp::SuperCall { args } => {
            frame()?;
            let ctor = super::classops::emit_get_super(b, ctx, "constructor")?;
            let r = emit_super_call(b, ctx, values, ctor, args)?;
            Ok(Some(Some(Out::Boxed(r))))
        }
        SsaOp::SuperMethodCall { name, args } => {
            frame()?;
            let m = super::classops::emit_get_super(b, ctx, name)?;
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(boxed_value(b, ctx, values, *a)?);
            }
            let w = stage_value(b, ctx, m, &vals);
            let (mt, mp) = b.ins().isplit(m);
            let r = super::call::emit_invoke(b, ctx, w, (mt, mp), vals.len() + 1);
            Ok(Some(Some(Out::Boxed(r))))
        }
        SsaOp::ExtensionCall {
            func,
            slot,
            recv,
            args,
        } => {
            let f = frame()?;
            let callee = match slot {
                Some(s) => super::globals::emit_load(b, ctx, *s, super::globals::Region::Module)?,
                None => {
                    let niv = b
                        .ins()
                        .iconst(types::I64, super::props::str_idx(ctx, func)? as i64);
                    call_helper_void(b, ctx.cc, h.load_global_by_name, &[ectx, f.closure, niv]);
                    native(b)
                }
            };
            let mut full = Vec::with_capacity(args.len() + 1);
            full.push(*recv);
            full.extend_from_slice(args);
            let w = super::call::boxed_window(b, ctx, values, callee, &full)?;
            let (ct, cp) = b.ins().isplit(callee);
            let r = super::call::emit_invoke(b, ctx, w, (ct, cp), full.len() + 1);
            Ok(Some(Some(Out::Boxed(r))))
        }
        SsaOp::CallSpread { callee, args } => {
            let c = load_value(b, ctx, values, *callee)?;
            let (ct, cp) = b.ins().isplit(c);
            let mut vals = Vec::with_capacity(args.len());
            for s in args {
                let v = boxed_value(b, ctx, values, s.value)?;
                if s.spread {
                    let (t, p) = b.ins().isplit(v);
                    call_helper_void(b, ctx.cc, h.wrap_spread, &[ectx, t, p]);
                    vals.push(native(b));
                } else {
                    vals.push(v);
                }
            }
            let (addr, _) = stage(b, ctx, &vals);
            let argc = b.ins().iconst(types::I64, vals.len() as i64);
            call_helper_void(
                b,
                ctx.cc,
                h.jit_call_spread_window,
                &[ectx, ct, cp, addr, argc],
            );
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::LoadModule { source, own_ip } => {
            let f = frame()?;
            spill_all(b, ctx, values)?;
            let idx = super::props::str_idx(ctx, source)? as i64;
            let siv = b.ins().iconst(types::I64, idx);
            let oiv = b.ins().iconst(types::I64, i64::from(*own_ip));
            call_helper_void(b, ctx.cc, h.load_module, &[ectx, f.closure, siv, oiv]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::ModuleSlot { object, slot } => {
            let o = boxed_value(b, ctx, values, *object)?;
            let (ot, op_) = b.ins().isplit(o);
            let siv = b.ins().iconst(types::I64, i64::from(*slot));
            call_helper_void(b, ctx.cc, h.load_module_slot, &[ectx, ot, op_, siv]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::StoreModuleSlot { slot, value } => {
            let v = boxed_value(b, ctx, values, *value)?;
            let (t, p) = b.ins().isplit(v);
            let siv = b.ins().iconst(types::I64, i64::from(*slot));
            call_helper_void(b, ctx.cc, h.store_module_slot, &[ectx, siv, t, p]);
            Ok(Some(None))
        }
        SsaOp::Await { operand, resume_ip } => {
            let d = dest.ok_or("from_ssa: await without dest")?;
            spill_all(b, ctx, values)?;
            let v = boxed_value(b, ctx, values, *operand)?;
            let (t, p) = b.ins().isplit(v);
            let dv = b.ins().iconst(types::I64, i64::from(ctx.ssa.reg(d)));
            let rv = b.ins().iconst(types::I64, i64::from(*resume_ip));
            // No trap: the helper never returns to compiled code (it parks
            // the frame and longjmps to the interpreter), but this is a
            // mid-block instruction — later instructions still need a home.
            // The block's own terminator closes it, as with any helper call.
            call_helper_void(b, ctx.cc, h.await_helper, &[ectx, t, p, dv, rv]);
            Ok(Some(None))
        }
        SsaOp::Spawn { operand } => {
            let v = boxed_value(b, ctx, values, *operand)?;
            let (t, p) = b.ins().isplit(v);
            call_helper_void(b, ctx.cc, h.spawn, &[ectx, t, p]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::Yield { operand, resume_ip } => {
            let d = dest.ok_or("from_ssa: yield without dest")?;
            spill_all(b, ctx, values)?;
            let v = boxed_value(b, ctx, values, *operand)?;
            let (t, p) = b.ins().isplit(v);
            let dv = b.ins().iconst(types::I64, i64::from(ctx.ssa.reg(d)));
            let rv = b.ins().iconst(types::I64, i64::from(*resume_ip));
            // No trap, as for `Await` above: a mid-block instruction must
            // leave the block open for its terminator.
            call_helper_void(b, ctx.cc, h.yield_helper, &[ectx, t, p, dv, rv]);
            Ok(Some(None))
        }
        SsaOp::Dispose { var, is_await, cs } => {
            frame()?;
            let reg = ctx
                .ssa
                .captured_reg(*var)
                .ok_or("from_ssa: dispose unknown var")?;
            let recv = use_heap(b, ctx, reg)?;
            let name = if *is_await { "disposeAsync" } else { "dispose" };
            let niv = b
                .ins()
                .iconst(types::I64, super::props::str_idx(ctx, name)? as i64);
            let csv = b.ins().iconst(types::I64, i64::from(*cs));
            let w = super::call::boxed_window(b, ctx, values, recv, &[])?;
            let total = b.ins().iconst(types::I64, 1);
            call_helper_void(
                b,
                ctx.cc,
                h.jit_call_method_window,
                &[ectx, niv, csv, w, total],
            );
            Ok(Some(None))
        }
        _ => Ok(None),
    }
}

/// `super(...args)`: the constructor through `GetSuper`, called with this
/// (home 0) then `args` — the bytecode's own window.
fn emit_super_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    ctor: Value,
    args: &[u32],
) -> Result<Value, String> {
    let this = use_heap(b, ctx, 0)?;
    let mut vals = Vec::with_capacity(args.len() + 1);
    vals.push(this);
    for a in args {
        vals.push(boxed_value(b, ctx, values, *a)?);
    }
    let addr = super::call::scratch_addr(b, ctx, vals.len().max(1));
    for (i, v) in vals.iter().enumerate() {
        b.ins()
            .store(MemFlags::trusted(), *v, addr, (i * 16) as i32);
    }
    let (ct, cp) = b.ins().isplit(ctor);
    Ok(super::call::emit_invoke(b, ctx, addr, (ct, cp), vals.len()))
}

/// Native-stack window with `callee` first, then `args`, in the shared
/// scratch window.
fn stage_value(b: &mut FunctionBuilder, ctx: &Ctx<'_>, callee: Value, args: &[Value]) -> Value {
    let addr = super::call::scratch_addr(b, ctx, args.len() + 1);
    b.ins().store(MemFlags::trusted(), callee, addr, 0);
    for (i, v) in args.iter().enumerate() {
        b.ins()
            .store(MemFlags::trusted(), *v, addr, ((i + 1) * 16) as i32);
    }
    addr
}

/// Spread-free array build shared by `BuildTuple`.
fn window_result(b: &mut FunctionBuilder, ctx: &Ctx<'_>, helper: usize, vals: &[Value]) -> Value {
    let ectx = exec_ctx(ctx);
    let (addr, n) = stage(b, ctx, vals);
    call_helper_void(b, ctx.cc, helper, &[ectx, addr, n]);
    b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    )
}

/// Native-stack window of boxed values: `(addr, count)`, staged in the
/// function's shared scratch window (see [`super::call::ScratchWin`]).
fn stage(b: &mut FunctionBuilder, ctx: &Ctx<'_>, vals: &[Value]) -> (Value, Value) {
    let addr = super::call::scratch_addr(b, ctx, vals.len().max(1));
    for (i, v) in vals.iter().enumerate() {
        b.ins()
            .store(MemFlags::trusted(), *v, addr, (i * 16) as i32);
    }
    (addr, b.ins().iconst(types::I64, vals.len() as i64))
}

/// Empty array then push/extend per element; the running array stays rooted
/// in `jit_native_result` across the element helpers.
fn emit_array_spread(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    elements: &[SsaSpread],
) -> Result<Value, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        16,
        4,
    ));
    let addr = b.ins().stack_addr(types::I64, slot, 0);
    let zero = b.ins().iconst(types::I64, 0);
    call_helper_void(b, ctx.cc, h.build_array_window, &[ectx, addr, zero]);
    let fresh = |b: &mut FunctionBuilder| {
        b.ins().load(
            types::I128,
            MemFlags::trusted(),
            ectx,
            h.jit_native_result_offset as i32,
        )
    };
    for s in elements {
        let arr = fresh(b);
        let (at, ap) = b.ins().isplit(arr);
        let v = boxed_value(b, ctx, values, s.value)?;
        let (vt, vp) = b.ins().isplit(v);
        if s.spread {
            call_helper_void(b, ctx.cc, h.array_extend, &[ectx, at, ap, vt, vp]);
        } else {
            call_helper_void(b, ctx.cc, h.array_push, &[ectx, at, ap, vt, vp]);
        }
    }
    Ok(fresh(b))
}

/// Empty object then set/merge per part; keyed parts use consecutive cache
/// slots from `cs_base`.
fn emit_object_spread(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    parts: &[SsaObjectSpreadPart],
    cs_base: u16,
) -> Result<Value, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: spread without a frame")?;
    call_helper_void(b, ctx.cc, h.build_empty_object, &[ectx]);
    let fresh = |b: &mut FunctionBuilder| {
        b.ins().load(
            types::I128,
            MemFlags::trusted(),
            ectx,
            h.jit_native_result_offset as i32,
        )
    };
    let mut keyed = 0u32;
    for p in parts {
        match &p.key {
            Some(k) => {
                let obj = fresh(b);
                let (ot, op_) = b.ins().isplit(obj);
                let v = boxed_value(b, ctx, values, p.value)?;
                let (vt, vp) = b.ins().isplit(v);
                let niv = b
                    .ins()
                    .iconst(types::I64, super::props::str_idx(ctx, k)? as i64);
                let csv = b
                    .ins()
                    .iconst(types::I64, i64::from(cs_base) + i64::from(keyed));
                let ipv = b.ins().iconst(types::I64, 0);
                call_helper_void(
                    b,
                    ctx.cc,
                    h.set_property_flat,
                    &[ectx, frame.closure, ot, op_, vt, vp, niv, csv, ipv],
                );
                // Setters are user code: frames may have moved underneath
                // the next part's home reads.
                super::store::drop_home_addrs(ctx);
                keyed += 1;
            }
            None => {
                let obj = fresh(b);
                let (ot, op_) = b.ins().isplit(obj);
                let v = boxed_value(b, ctx, values, p.value)?;
                let (vt, vp) = b.ins().isplit(v);
                call_helper_void(b, ctx.cc, h.object_merge, &[ectx, ot, op_, vt, vp]);
            }
        }
    }
    Ok(fresh(b))
}

/// Every defined scalar to its home, so a suspending helper's interpreter
/// resume reads the same registers compiled code held natively.
fn spill_all(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
) -> Result<(), String> {
    for v in 0..ctx.ssa.values.len() as u32 {
        if super::store::is_heap(ctx.ssa.value_ty(v)) {
            continue;
        }
        let Ok(x) = load_value(b, ctx, values, v) else {
            continue;
        };
        let boxed = match ctx.ssa.value_ty(v) {
            varn_types::register_meta::SlotKind::Int => super::super::emit::box_int(b, x),
            varn_types::register_meta::SlotKind::Float => super::super::emit::box_f64(b, x),
            varn_types::register_meta::SlotKind::Bool => super::super::emit::box_bool(b, x),
            _ => continue,
        };
        def_heap(b, ctx, ctx.ssa.reg(v), boxed)?;
    }
    Ok(())
}
