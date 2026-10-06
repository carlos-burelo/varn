








use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::{SsaObjectSpreadPart, SsaOp, SsaSpread};

use super::heap::{boxed_value, exec_ctx};
use super::store::{home_load, home_store, Out};
use super::{load_value, Ctx};

use super::super::emit::{call_helper, call_helper_void};




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
            cranelift_codegen::ir::MemFlagsData::trusted(),
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
        SsaOp::LoadModule {
            source,
            own_ip,
            live,
        } => {
            let f = frame()?;
            spill(b, ctx, values, live)?;
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
        SsaOp::Await {
            operand,
            resume_ip,
            live,
        } => {
            let d = dest.ok_or("from_ssa: await without dest")?;
            spill(b, ctx, values, live)?;
            let v = boxed_value(b, ctx, values, *operand)?;
            let (t, p) = b.ins().isplit(v);
            let dv = b.ins().iconst(types::I64, i64::from(ctx.ssa.reg(d)));
            let rv = b.ins().iconst(types::I64, i64::from(*resume_ip));
            
            
            
            
            call_helper_void(b, ctx.cc, h.await_helper, &[ectx, t, p, dv, rv]);
            Ok(Some(None))
        }
        SsaOp::Spawn { operand } => {
            let v = boxed_value(b, ctx, values, *operand)?;
            let (t, p) = b.ins().isplit(v);
            call_helper_void(b, ctx.cc, h.spawn, &[ectx, t, p]);
            Ok(Some(Some(Out::Boxed(native(b)))))
        }
        SsaOp::Yield {
            operand,
            resume_ip,
            live,
        } => {
            let d = dest.ok_or("from_ssa: yield without dest")?;
            spill(b, ctx, values, live)?;
            let v = boxed_value(b, ctx, values, *operand)?;
            let (t, p) = b.ins().isplit(v);
            let dv = b.ins().iconst(types::I64, i64::from(ctx.ssa.reg(d)));
            let rv = b.ins().iconst(types::I64, i64::from(*resume_ip));
            
            
            call_helper_void(b, ctx.cc, h.yield_helper, &[ectx, t, p, dv, rv]);
            Ok(Some(None))
        }
        SsaOp::Dispose { var, is_await, cs } => {
            frame()?;
            let reg = ctx
                .ssa
                .captured_reg(*var)
                .ok_or("from_ssa: dispose unknown var")?;
            let recv = home_load(b, ctx, reg)?;
            let name = if *is_await { "disposeAsync" } else { "dispose" };
            let niv = b
                .ins()
                .iconst(types::I64, super::props::str_idx(ctx, name)? as i64);
            let csv = b.ins().iconst(types::I64, i64::from(*cs));
            let w = super::call::boxed_window(b, ctx, values, recv, &[])?;
            let total = b.ins().iconst(types::I64, 1);
            let out = super::call::entry_out_slot(b);
            let entry = call_helper(
                b,
                ctx.cc,
                h.jit_call_method_window,
                &[ectx, niv, csv, w, total, out],
            );
            super::call::run_entered_or(b, ctx, entry, out);
            Ok(Some(None))
        }
        _ => Ok(None),
    }
}



fn emit_super_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    ctor: Value,
    args: &[u32],
) -> Result<Value, String> {
    let this = super::props::emit_this(ctx)?;
    let mut vals = Vec::with_capacity(args.len() + 1);
    vals.push(this);
    for a in args {
        vals.push(boxed_value(b, ctx, values, *a)?);
    }
    let addr = super::call::scratch_addr(b, ctx, vals.len().max(1));
    for (i, v) in vals.iter().enumerate() {
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            *v,
            addr,
            (i * 16) as i32,
        );
    }
    let (ct, cp) = b.ins().isplit(ctor);
    Ok(super::call::emit_invoke(b, ctx, addr, (ct, cp), vals.len()))
}



fn stage_value(b: &mut FunctionBuilder, ctx: &Ctx<'_>, callee: Value, args: &[Value]) -> Value {
    let addr = super::call::scratch_addr(b, ctx, args.len() + 1);
    b.ins().store(
        cranelift_codegen::ir::MemFlagsData::trusted(),
        callee,
        addr,
        0,
    );
    for (i, v) in args.iter().enumerate() {
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            *v,
            addr,
            ((i + 1) * 16) as i32,
        );
    }
    addr
}


fn window_result(b: &mut FunctionBuilder, ctx: &Ctx<'_>, helper: usize, vals: &[Value]) -> Value {
    let ectx = exec_ctx(ctx);
    let (addr, n) = stage(b, ctx, vals);
    call_helper_void(b, ctx.cc, helper, &[ectx, addr, n]);
    b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    )
}



fn stage(b: &mut FunctionBuilder, ctx: &Ctx<'_>, vals: &[Value]) -> (Value, Value) {
    let addr = super::call::scratch_addr(b, ctx, vals.len().max(1));
    for (i, v) in vals.iter().enumerate() {
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            *v,
            addr,
            (i * 16) as i32,
        );
    }
    (addr, b.ins().iconst(types::I64, vals.len() as i64))
}



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
            cranelift_codegen::ir::MemFlagsData::trusted(),
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
            cranelift_codegen::ir::MemFlagsData::trusted(),
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



fn spill(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    live: &[u32],
) -> Result<(), String> {
    for &v in live {
        let Ok(x) = load_value(b, ctx, values, v) else {
            continue;
        };
        let boxed = match ctx.ssa.value_ty(v) {
            varn_types::register_meta::SlotKind::Int => super::super::emit::box_int(b, x),
            varn_types::register_meta::SlotKind::Float => super::super::emit::box_f64(b, x),
            varn_types::register_meta::SlotKind::Bool => super::super::emit::box_bool(b, x),
            varn_types::register_meta::SlotKind::Str
            | varn_types::register_meta::SlotKind::Ref
            | varn_types::register_meta::SlotKind::Dynamic => x,
        };
        home_store(b, ctx, ctx.ssa.reg(v), boxed)?;
    }
    Ok(())
}
