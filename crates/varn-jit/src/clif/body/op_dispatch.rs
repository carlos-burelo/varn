//! Opcode lowering dispatch module for CLIF body compilation.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_core::OpCode;
use varn_types::chunk::PoolEntry;
use varn_types::register_meta::SlotKind;
use varn_types::{FunctionProto, VmValue};

use super::super::alloc::{self, AllocCtx};
use super::super::arrays;
use super::super::emit::{
    box_f64, box_int, box_or_pass, call_helper, def_const, def_const_bool, def_const_int,
    emit_return_value, guard_overflow, meta_is_float, state_meta_int, unbox_bool,
    unbox_f64_coerce, use_boxed, use_f64, use_int,
};
use super::super::fields;
use super::super::floats;
use super::super::generic;
use super::super::globals;
use super::super::kinds::K;
use super::super::lower::ClifLinker;
use super::super::methods;
use crate::JitHelpers;

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_opcode(
    b: &mut FunctionBuilder,
    op: OpCode,
    code: &[u16],
    pool: &[PoolEntry],
    ip: usize,
    first_reg: usize,
    proto: &FunctionProto,
    constants: &[VmValue],
    vars: &[Variable],
    state: &mut [K],
    actx: Option<&AllocCtx>,
    exec_ctx: cranelift_codegen::ir::Value,
    helpers: &JitHelpers,
    cc: CallConv,
    _has_alloc: bool,
    has_round: bool,
    linker: &dyn ClifLinker,
    arr: &arrays::ArrCtx,
    fld: &fields::FldCtx,
    gbl: &globals::GblCtx,
    gen: &generic::GenCtx,
    entry: cranelift_codegen::ir::Block,
    self_ref: cranelift_codegen::ir::FuncRef,
    frame_aware: bool,
    osr: bool,
    nparams: usize,
) -> Result<bool, String> {
    match op {
        OpCode::LoadIntZero => def_const_int(b, &proto.register_meta, vars, first_reg, 0),
        OpCode::LoadIntOne => def_const_int(b, &proto.register_meta, vars, first_reg, 1),
        OpCode::LoadIntMinusOne => def_const_int(b, &proto.register_meta, vars, first_reg, -1),
        OpCode::LoadTrue => def_const_bool(b, vars, first_reg, true),
        OpCode::LoadFalse => def_const_bool(b, vars, first_reg, false),
        OpCode::LoadInt => {
            let v = code[ip + 1] as i16 as i64;
            def_const_int(b, &proto.register_meta, vars, first_reg, v);
        }
        OpCode::LoadConst => {
            let idx = code[ip + 1] as usize;
            let c = *constants.get(idx).ok_or("clif: constant index")?;
            if meta_is_float(&proto.register_meta, first_reg) {
                let f = if c.is_f64() {
                    b.ins().f64const(c.as_f64())
                } else if c.is_int() {
                    b.ins().f64const(c.as_int() as f64)
                } else {
                    return Err("clif: non-numeric const into float reg".into());
                };
                b.def_var(vars[first_reg], f);
            } else if c.is_int() {
                def_const(b, vars, first_reg, c.as_int());
            } else if c.is_heap() && (c.as_heap_idx() & 0x8000_0000 == 0) {
                return Err("clif: nursery heap constant".into());
            } else {
                def_const(b, vars, first_reg, c.0 as i64);
            }
        }
        OpCode::LoadNull => {
            def_const(b, vars, first_reg, VmValue::null().0 as i64);
        }
        OpCode::Move => {
            let src = (code[ip + 1] >> 8) as usize;
            let src_is_float = meta_is_float(&proto.register_meta, src);
            let dest_is_float = meta_is_float(&proto.register_meta, first_reg);
            if first_reg < vars.len() && src < vars.len() {
                let val_to_store = if dest_is_float && !src_is_float {
                    let v = box_or_pass(b, vars, state, src);
                    let f = unbox_f64_coerce(b, v);
                    b.def_var(vars[first_reg], f);
                    v
                } else if !dest_is_float && src_is_float {
                    let f = b.use_var(vars[src]);
                    let boxed = box_f64(b, f);
                    b.def_var(vars[first_reg], boxed);
                    boxed
                } else {
                    let v = b.use_var(vars[src]);
                    b.def_var(vars[first_reg], v);
                    box_or_pass(b, vars, state, src)
                };
                if let Some(actx) = actx {
                    let fb = alloc::frame_base_addr(b, actx);
                    b.ins().store(
                        MemFlags::trusted(),
                        val_to_store,
                        fb,
                        (first_reg * 8) as i32,
                    );
                }
            }
        }

        OpCode::AddInt | OpCode::SubInt | OpCode::MulInt => {
            let w1 = code[ip + 1];
            let (r1, r2) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
            let s1 = use_int(b, vars, state, r1)?;
            let s2 = use_int(b, vars, state, r2)?;
            let (r, overflow, helper) = match op {
                OpCode::AddInt => {
                    let (res, ovf) = b.ins().sadd_overflow(s1, s2);
                    (res, ovf, helpers.add)
                }
                OpCode::SubInt => {
                    let (res, ovf) = b.ins().ssub_overflow(s1, s2);
                    (res, ovf, helpers.sub)
                }
                _ => {
                    let (res, ovf) = b.ins().smul_overflow(s1, s2);
                    (res, ovf, helpers.mul)
                }
            };
            let w = guard_overflow(b, cc, exec_ctx, helper, r, overflow, s1, s2);
            b.def_var(vars[first_reg], w);
        }
        OpCode::Negate => {
            let src = (code[ip + 1] >> 8) as usize;
            if meta_is_float(&proto.register_meta, first_reg) || state[src] == K::Float {
                let f = use_f64(b, vars, state, src)?;
                let neg = b.ins().fneg(f);
                if meta_is_float(&proto.register_meta, first_reg) {
                    b.def_var(vars[first_reg], neg);
                } else {
                    let boxed = box_f64(b, neg);
                    b.def_var(vars[first_reg], boxed);
                }
            } else if state[src] == K::Int || state_meta_int(&proto.register_meta, first_reg) {
                let i = use_int(b, vars, state, src)?;
                let neg = b.ins().ineg(i);
                let fits = b.ins().icmp_imm(IntCC::NotEqual, i, i64::MIN);
                let raise = b.create_block();
                let cont = b.create_block();
                b.ins().brif(fits, cont, &[], raise, &[]);
                b.switch_to_block(raise);
                let boxed = box_int(b, i);
                let _ = call_helper(b, cc, helpers.negate, &[exec_ctx, boxed]);
                b.ins().jump(cont, &[]);
                b.switch_to_block(cont);
                if state_meta_int(&proto.register_meta, first_reg) {
                    b.def_var(vars[first_reg], neg);
                } else {
                    let boxed = box_int(b, neg);
                    b.def_var(vars[first_reg], boxed);
                }
            } else {
                let actx = actx.ok_or("clif: Negate outside alloc fn")?;
                let regs = alloc::live_boxed(actx, state);
                alloc::flush_boxed(b, actx, state, &regs);
                let v = box_or_pass(b, vars, state, src);
                let res = call_helper(b, cc, helpers.negate, &[exec_ctx, v]);
                alloc::reload_boxed(b, actx, state, &regs);
                alloc::def_result(b, actx, first_reg, res);
            }
        }
        OpCode::BitAnd
        | OpCode::BitOr
        | OpCode::BitXor
        | OpCode::Shl
        | OpCode::Shr
        | OpCode::Ushr => {
            let w1 = code[ip + 1];
            let (r1, r2) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
            let a = use_int(b, vars, state, r1)?;
            let c = use_int(b, vars, state, r2)?;
            let r = match op {
                OpCode::BitAnd => b.ins().band(a, c),
                OpCode::BitOr => b.ins().bor(a, c),
                OpCode::BitXor => b.ins().bxor(a, c),
                OpCode::Shl => {
                    let shift = b.ins().band_imm(c, 0x3F);
                    b.ins().ishl(a, shift)
                }
                OpCode::Shr => {
                    let shift = b.ins().band_imm(c, 0x3F);
                    b.ins().sshr(a, shift)
                }
                OpCode::Ushr => {
                    let shift = b.ins().band_imm(c, 0x3F);
                    b.ins().ushr(a, shift)
                }
                _ => unreachable!(),
            };
            if state_meta_int(&proto.register_meta, first_reg) {
                b.def_var(vars[first_reg], r);
            } else {
                let boxed = box_int(b, r);
                b.def_var(vars[first_reg], boxed);
            }
        }
        OpCode::AddImm | OpCode::SubImm => {
            let w1 = code[ip + 1];
            let src = (w1 >> 8) as usize;
            let imm = (w1 & 0xFF) as i8 as i64;
            let s = use_int(b, vars, state, src)?;
            let imm_v = b.ins().iconst(types::I64, imm);
            let (r, overflow, helper) = if op == OpCode::AddImm {
                let (res, ovf) = b.ins().sadd_overflow(s, imm_v);
                (res, ovf, helpers.add)
            } else {
                let (res, ovf) = b.ins().ssub_overflow(s, imm_v);
                (res, ovf, helpers.sub)
            };
            let w = guard_overflow(b, cc, exec_ctx, helper, r, overflow, s, imm_v);
            b.def_var(vars[first_reg], w);
        }
        OpCode::ModInt => {
            let w1 = code[ip + 1];
            let (r1, r2) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
            let a = use_int(b, vars, state, r1)?;
            let d = use_int(b, vars, state, r2)?;
            let is_zero = b.ins().icmp_imm(IntCC::Equal, d, 0);
            let raise = b.create_block();
            let ok = b.create_block();
            let merge = b.create_block();
            b.append_block_param(merge, types::I64);
            b.ins().brif(is_zero, raise, &[], ok, &[]);
            b.switch_to_block(raise);
            let ba = box_int(b, a);
            let bd = box_int(b, d);
            let _ = call_helper(b, cc, helpers.modulo, &[exec_ctx, ba, bd]);
            let z = b.ins().iconst(types::I64, 0);
            b.ins().jump(merge, &[z.into()]);
            b.switch_to_block(ok);
            let r = b.ins().srem(a, d);
            b.ins().jump(merge, &[r.into()]);
            b.switch_to_block(merge);
            let res = b.block_params(merge)[0];
            b.def_var(vars[first_reg], res);
        }
        OpCode::LtInt
        | OpCode::LteInt
        | OpCode::GtInt
        | OpCode::GteInt
        | OpCode::EqInt
        | OpCode::NeqInt => {
            let w1 = code[ip + 1];
            let (r1, r2) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
            if state[r1] == K::Int && state[r2] == K::Int {
                let s1 = use_int(b, vars, state, r1)?;
                let s2 = use_int(b, vars, state, r2)?;
                let int_cc = match op {
                    OpCode::LtInt => IntCC::SignedLessThan,
                    OpCode::LteInt => IntCC::SignedLessThanOrEqual,
                    OpCode::GtInt => IntCC::SignedGreaterThan,
                    OpCode::GteInt => IntCC::SignedGreaterThanOrEqual,
                    OpCode::EqInt => IntCC::Equal,
                    OpCode::NeqInt => IntCC::NotEqual,
                    _ => unreachable!(),
                };
                let c = b.ins().icmp(int_cc, s1, s2);
                let ext = b.ins().uextend(types::I64, c);
                b.def_var(vars[first_reg], ext);
            } else {
                let h_fn = match op {
                    OpCode::LtInt => helpers.lt,
                    OpCode::LteInt => helpers.lte,
                    OpCode::GtInt => helpers.gt,
                    OpCode::GteInt => helpers.gte,
                    OpCode::EqInt => helpers.eq,
                    OpCode::NeqInt => helpers.neq,
                    _ => unreachable!(),
                };
                generic::emit_compare(b, gen, state, code, ip, h_fn);
            }
        }

        OpCode::Return => {
            let src = (code[ip + 1] & 0xFF) as usize;
            let v = emit_return_value(b, vars, state, proto.return_kind, src)?;
            b.ins().return_(&[v]);
            return Ok(true);
        }
        OpCode::CallSelf => {
            let w1 = code[ip + 1];
            let w2 = code[ip + 2];
            let dest = (w1 >> 8) as usize;
            let arg_count = (w2 >> 8) as usize;
            let arg_start = (w2 & 0xFF) as usize;
            if arg_count != nparams + 1 {
                return Err("clif: CallSelf arity mismatch".into());
            }
            if osr {
                return Err("osr: CallSelf cannot target the resume entry".into());
            }
            let mut args = Vec::with_capacity(4 + nparams);
            if frame_aware {
                let stack_ptr = b.block_params(entry)[0];
                let closure_val = b.block_params(entry)[1];
                let base_val = b.block_params(entry)[2];
                args.push(stack_ptr);
                args.push(closure_val);
                args.push(base_val);
            }
            args.push(exec_ctx);
            for i in 0..nparams {
                let r = arg_start + 1 + i;
                let v = if proto.param_kinds.get(i) == Some(&SlotKind::Int) {
                    use_int(b, vars, state, r)?
                } else if proto.param_kinds.get(i) == Some(&SlotKind::Float) {
                    if meta_is_float(&proto.register_meta, r) || state[r] == K::Float {
                        let f = use_f64(b, vars, state, r)?;
                        b.ins().bitcast(types::I64, MemFlags::trusted(), f)
                    } else {
                        let boxed = use_boxed(b, vars, state, r)?;
                        let f = unbox_f64_coerce(b, boxed);
                        b.ins().bitcast(types::I64, MemFlags::trusted(), f)
                    }
                } else if proto.param_kinds.get(i) == Some(&SlotKind::Bool) {
                    let boxed = use_boxed(b, vars, state, r)?;
                    unbox_bool(b, boxed)
                } else {
                    use_boxed(b, vars, state, r)?
                };
                args.push(v);
            }
            let call = b.ins().call(self_ref, &args);
            let res = b.inst_results(call)[0];
            if meta_is_float(&proto.register_meta, dest) {
                let f = unbox_f64_coerce(b, res);
                b.def_var(vars[dest], f);
            } else {
                b.def_var(vars[dest], res);
            }
        }

        OpCode::LoadGlobalIdx => {
            globals::emit_load_global_idx(b, gbl, code, ip, first_reg);
        }
        OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
            globals::emit_store_global_idx(b, gbl, state, code, ip)?;
        }
        OpCode::ArrayLength => {
            arrays::emit_array_length(b, arr, state, code, ip, first_reg)?;
        }
        OpCode::ArrayGetIndex => {
            arrays::emit_array_get_index(b, arr, state, code, ip, first_reg)?;
        }
        OpCode::ArraySetIndex => {
            arrays::emit_array_set_index(b, arr, state, code, ip, first_reg)?;
        }
        OpCode::GetFixedField => {
            fields::emit_get_fixed_field(b, fld, state, code, ip, first_reg)?;
        }
        OpCode::GetProperty => {
            let actx = actx.ok_or("clif: GetProperty outside frame-aware fn")?;
            alloc::emit_get_property(b, actx, state, &proto.register_meta, code, ip);
        }
        OpCode::SetProperty => {
            let actx = actx.ok_or("clif: SetProperty outside frame-aware fn")?;
            alloc::emit_set_property(b, actx, state, code, ip);
        }
        OpCode::SetFixedField => {
            fields::emit_set_fixed_field(b, fld, state, code, ip, first_reg)?;
        }
        OpCode::Call => {
            let actx = actx.ok_or("clif: call in non-frame-aware fn")?;
            let callee_reg = (code[ip + 1] & 0xFF) as usize;
            let target = match state[callee_reg] {
                K::Global(i) => linker.static_target(i as usize),
                _ => None,
            };
            alloc::emit_call(b, actx, state, code, ip, target.as_ref())?;
        }
        OpCode::BuildArray | OpCode::BuildTuple => {
            let actx = actx.ok_or("clif: BuildArray outside alloc fn")?;
            alloc::emit_build_array(b, actx, state, code, ip);
        }
        OpCode::ArrayPush => {
            let actx = actx.ok_or("clif: ArrayPush outside alloc fn")?;
            alloc::emit_array_push(b, actx, state, code, ip);
        }
        OpCode::StrConcat => {
            let actx = actx.ok_or("clif: StrConcat outside alloc fn")?;
            super::super::strconcat::emit_str_concat(b, actx, state, code, ip);
        }
        OpCode::CallNativeOp => {
            let actx = actx.ok_or("clif: CallNativeOp outside alloc fn")?;
            alloc::emit_call_native_op(b, actx, state, &proto.register_meta, code, pool, ip)?;
        }
        OpCode::CallMethod => {
            let actx = actx.ok_or("clif: CallMethod outside alloc fn")?;
            methods::emit_call_method(b, actx, state, proto, code, ip);
        }
        OpCode::InvokeVirtual => {
            let actx = actx.ok_or("clif: InvokeVirtual outside alloc fn")?;
            methods::emit_invoke_virtual(b, actx, state, proto, code, ip);
        }
        OpCode::MakeEnumVariant => {
            let actx = actx.ok_or("clif: MakeEnumVariant outside alloc fn")?;
            alloc::emit_make_enum_variant(b, actx, state, code, ip);
        }
        OpCode::GetEnumTag => {
            let actx = actx.ok_or("clif: GetEnumTag outside alloc fn")?;
            alloc::emit_get_enum_tag(b, actx, state, &proto.register_meta, code, ip);
        }
        OpCode::BuildStr => {
            let actx = actx.ok_or("clif: BuildStr outside alloc fn")?;
            alloc::emit_build_str(b, actx, state, code, ip);
        }
        OpCode::Intrinsic | OpCode::IntrinsicDirect => {
            let m = &proto.register_meta;
            floats::emit_intrinsic_op(b, op, actx, arr.loops, vars, state, m, code, ip, has_round)?;
        }
        OpCode::ToString => {
            let actx = actx.ok_or("clif: ToString outside alloc fn")?;
            alloc::emit_to_string(b, actx, state, &proto.register_meta, code, ip);
        }
        OpCode::BuildObjectWithShape | OpCode::BuildRecord => {
            let actx = actx.ok_or("clif: BuildObjectWithShape outside alloc fn")?;
            let shape_idx = code[ip + 2] as usize;
            let count = proto
                .resolved_shape(shape_idx)
                .map(|s| s.property_names.len())
                .ok_or("clif: unresolved object shape")?;
            let is_record = op == OpCode::BuildRecord;
            alloc::emit_build_object_with_shape(b, actx, state, code, ip, count, is_record);
        }
        OpCode::LoadUpvalue => {
            let actx = actx.ok_or("clif: LoadUpvalue outside frame-aware fn")?;
            alloc::emit_load_upvalue(b, actx, code, ip);
        }
        OpCode::StoreUpvalue => {
            let actx = actx.ok_or("clif: StoreUpvalue outside frame-aware fn")?;
            alloc::emit_store_upvalue(b, actx, state, code, ip);
        }
        OpCode::CloseUpvalue => {
            let actx = actx.ok_or("clif: CloseUpvalue outside frame-aware fn")?;
            alloc::emit_close_upvalue(b, actx, state, code, ip);
        }
        OpCode::MakeClosure => {
            let actx = actx.ok_or("clif: MakeClosure outside alloc fn")?;
            alloc::emit_make_closure(b, actx, state, code, ip);
        }
        OpCode::LoadStaticFn => {
            let actx = actx.ok_or("clif: LoadStaticFn outside alloc fn")?;
            alloc::emit_load_static_fn(b, actx, state, code, ip);
        }
        OpCode::LoadModule => {
            let actx = actx.ok_or("clif: LoadModule outside alloc fn")?;
            alloc::emit_load_module(b, actx, state, code, ip);
        }
        OpCode::LoadModuleSlot => {
            let actx = actx.ok_or("clif: LoadModuleSlot outside alloc fn")?;
            alloc::emit_load_module_slot(b, actx, state, code, ip);
        }
        OpCode::StoreModuleSlot => {
            let actx = actx.ok_or("clif: StoreModuleSlot outside alloc fn")?;
            alloc::emit_store_module_slot(b, actx, state, code, ip);
        }
        OpCode::MakeClass => {
            let actx = actx.ok_or("clif: MakeClass outside alloc fn")?;
            super::super::classes::emit_make_class(b, actx, state, code, ip);
        }
        OpCode::DeclareField
        | OpCode::Method
        | OpCode::DefineStatic
        | OpCode::DefineGetter
        | OpCode::DefineSetter
        | OpCode::DefineStaticGetter
        | OpCode::DefineStaticSetter
        | OpCode::Inherit => {
            let actx = actx.ok_or("clif: ClassMemberOp outside alloc fn")?;
            super::super::classes::emit_class_member_op(b, actx, state, op, code, ip)?;
        }
        OpCode::GetSuper => {
            let actx = actx.ok_or("clif: GetSuper outside alloc fn")?;
            super::super::classes::emit_get_super(b, actx, state, code, ip);
        }
        OpCode::GetIndex => {
            let actx = actx.ok_or("clif: GetIndex outside alloc fn")?;
            alloc::emit_get_index(b, actx, state, code, ip);
        }
        OpCode::SetIndex => {
            let actx = actx.ok_or("clif: SetIndex outside alloc fn")?;
            alloc::emit_set_index(b, actx, state, code, ip);
        }
        OpCode::Try => {
            let actx = actx.ok_or("clif: Try outside alloc fn")?;
            alloc::emit_try_push(b, actx, code, ip);
        }
        OpCode::PopTry => {
            let actx = actx.ok_or("clif: PopTry outside alloc fn")?;
            alloc::emit_try_pop(b, actx);
        }
        OpCode::Throw => {
            let actx = actx.ok_or("clif: Throw outside alloc fn")?;
            alloc::emit_throw(b, actx, state, code, ip);
        }
        OpCode::Yield => {
            let actx = actx.ok_or("clif: Yield outside alloc fn")?;
            alloc::emit_yield(b, actx, state, code, ip);
        }
        OpCode::Await => {
            let actx = actx.ok_or("clif: Await outside alloc fn")?;
            alloc::emit_await(b, actx, state, code, ip);
        }
        OpCode::Spawn => {
            let actx = actx.ok_or("clif: Spawn outside alloc fn")?;
            alloc::emit_spawn(b, actx, state, code, ip);
        }
        OpCode::BuildObject => {
            let actx = actx.ok_or("clif: BuildObject outside alloc fn")?;
            alloc::emit_build_object(b, actx, state, code, ip);
        }
        OpCode::ObjectRest => {
            let actx = actx.ok_or("clif: ObjectRest outside alloc fn")?;
            alloc::emit_object_rest(b, actx, state, code, ip);
        }
        OpCode::ObjectKeys => {
            let actx = actx.ok_or("clif: ObjectKeys outside alloc fn")?;
            alloc::emit_object_keys(b, actx, state, code, ip);
        }
        OpCode::ObjectMerge => {
            let actx = actx.ok_or("clif: ObjectMerge outside alloc fn")?;
            alloc::emit_object_merge(b, actx, state, code, ip);
        }
        OpCode::GetPropertyMaybe => {
            let actx = actx.ok_or("clif: GetPropertyMaybe outside alloc fn")?;
            alloc::emit_get_property_maybe(b, actx, state, code, ip);
        }
        OpCode::BindMethod => {
            let actx = actx.ok_or("clif: BindMethod outside alloc fn")?;
            alloc::emit_bind_method(b, actx, state, code, ip);
        }
        OpCode::AssertNotNull => {
            let actx = actx.ok_or("clif: AssertNotNull outside alloc fn")?;
            alloc::emit_assert_not_null(b, actx, state, code, ip);
        }
        OpCode::ArrayExtend => {
            let actx = actx.ok_or("clif: ArrayExtend outside alloc fn")?;
            alloc::emit_array_extend(b, actx, state, code, ip);
        }
        OpCode::WrapSpread => {
            let actx = actx.ok_or("clif: WrapSpread outside alloc fn")?;
            alloc::emit_wrap_spread(b, actx, state, code, ip);
        }
        OpCode::CallSpread => {
            let actx = actx.ok_or("clif: CallSpread outside alloc fn")?;
            alloc::emit_call_spread(b, actx, state, &proto.register_meta, code, ip);
        }
        OpCode::InvokeRuntimeStatic => {
            let actx = actx.ok_or("clif: InvokeRuntimeStatic outside alloc fn")?;
            alloc::emit_invoke_runtime_static(b, actx, state, code, ip)?;
        }
        OpCode::Nop => {}
        OpCode::AddFloat
        | OpCode::SubFloat
        | OpCode::MulFloat
        | OpCode::DivFloat
        | OpCode::ModFloat
        | OpCode::PowFloat
        | OpCode::LtFloat
        | OpCode::GtFloat
        | OpCode::LteFloat
        | OpCode::GteFloat
        | OpCode::EqFloat
        | OpCode::NeqFloat => {
            if !floats::emit_float_op(
                b,
                vars,
                state,
                &proto.register_meta,
                code,
                ip,
                op,
                cc,
                exec_ctx,
                helpers,
            )? && !generic::try_emit(b, gen, helpers, state, op, code, ip)
            {
                return Err(format!("clif: unsupported opcode {op:?}"));
            }
        }
        _ if generic::try_emit(b, gen, helpers, state, op, code, ip) => {}
        _ => return Err(format!("clif: unsupported opcode {op:?}")),
    }
    Ok(false)
}
