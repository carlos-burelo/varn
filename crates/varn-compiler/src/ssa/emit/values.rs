//! Instructions that define a value: constants, arithmetic, calls, property
//! and index reads, allocation, closures.

use super::super::ir::{BlockId, Inst, InstKind, VarId};
use super::regs::var_reg;
use super::terminator::emit_call_args;
use crate::hir::{HirUnOp, HirUpvalueSrc};
use crate::lower::bin_opcode;
use crate::OptError;
use std::rc::Rc;
use varn_core::OpCode;
use varn_types::chunk::{Chunk, Literal, PoolEntry};
use varn_types::value::RuntimeSymbol;

type Result<T> = std::result::Result<T, OptError>;

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_value(
    chunk: &mut Chunk,
    inst: &Inst,
    d: u8,
    value_tys: &[crate::hir::HirType],
    reg: &[u8],
    scratch: u8,
    call_base: u8,
    cache_count: &mut u16,
    source_file: &Rc<str>,
    nparams: usize,
    fixups: &mut Vec<(usize, BlockId)>,
) -> Result<()> {
    let line = inst.line;
    match &inst.kind {
        InstKind::ConstInt(n) => chunk.emit_load_int(d, *n, line),
        InstKind::ConstFloat(f) => {
            let idx = chunk.add_constant(PoolEntry::Literal(Literal::Float(*f)));
            chunk.emit_rc(OpCode::LoadConst, d, idx, line);
        }
        InstKind::ConstBool(b) => {
            let op = if *b {
                OpCode::LoadTrue
            } else {
                OpCode::LoadFalse
            };
            chunk.emit_rr(op, d, 0, line);
        }
        InstKind::ConstStr(s) => {
            let idx = chunk.add_str(s);
            chunk.emit_rc(OpCode::LoadConst, d, idx, line);
        }
        InstKind::ConstChar(c) => {
            let idx = chunk.add_constant(PoolEntry::Literal(Literal::Char(*c)));
            chunk.emit_rc(OpCode::LoadConst, d, idx, line);
        }
        InstKind::ConstDecimal(dec) => {
            let idx = chunk.add_constant(PoolEntry::Literal(Literal::Decimal(*dec)));
            chunk.emit_rc(OpCode::LoadConst, d, idx, line);
        }
        InstKind::ConstBigInt(n) => {
            let idx = chunk.add_constant(PoolEntry::Literal(Literal::BigInt(*n)));
            chunk.emit_rc(OpCode::LoadConst, d, idx, line);
        }
        InstKind::ConstNull => chunk.emit_rr(OpCode::LoadNull, d, 0, line),
        InstKind::Binary { op, lhs, rhs, ty } => {
            // `+` with a statically-proven string operand IS concatenation.
            // That is exactly what `arith::add` works out at RUN TIME, one
            // type test at a time, on every single execution — and the
            // checker already proved it here. The binary's own `ty` is
            // `Dynamic` for the common `"literal" + int`, so specializing on
            // the result type alone never reaches this.
            let str_operand = matches!(op, crate::hir::HirBinOp::Add)
                && (matches!(
                    value_tys.get(lhs.0 as usize),
                    Some(crate::hir::HirType::Str)
                ) || matches!(
                    value_tys.get(rhs.0 as usize),
                    Some(crate::hir::HirType::Str)
                ));
            let opcode = if str_operand {
                OpCode::StrConcat
            } else {
                bin_opcode(*op, *ty)
            };
            chunk.emit_rrr(opcode, d, reg[lhs.0 as usize], reg[rhs.0 as usize], line);
        }
        InstKind::Unary { op, operand, .. } => {
            let s = reg[operand.0 as usize];
            match op {
                HirUnOp::Neg => chunk.emit_rr(OpCode::Negate, d, s, line),
                HirUnOp::Not => chunk.emit_rr(OpCode::Not, d, s, line),
                HirUnOp::Typeof => chunk.emit_rr(OpCode::Typeof, d, s, line),
                HirUnOp::BitNot => {
                    let idx = chunk.add_constant(PoolEntry::Literal(Literal::Int(-1)));
                    chunk.emit_rc(OpCode::LoadConst, scratch, idx, line);
                    chunk.emit_rrr(OpCode::BitXor, d, s, scratch, line);
                }
            }
        }
        InstKind::LoadGlobal(name) => {
            let idx = chunk.add_str(name);
            chunk.emit_rc(OpCode::LoadGlobal, d, idx, line);
        }
        InstKind::LoadUpvalue(uv) => {
            chunk.emit(OpCode::LoadUpvalue, line);
            chunk.write(Chunk::pack(d, *uv as u8), line);
        }

        InstKind::Call { callee, args } => {
            emit_call_args(chunk, reg, call_base, args, line);
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::Call, line);
            chunk.write(Chunk::pack(d, reg[callee.0 as usize]), line);
            chunk.write(Chunk::pack(total, call_base), line);
        }

        InstKind::SelfCall { args } => {
            emit_call_args(chunk, reg, call_base, args, line);
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::CallSelf, line);
            chunk.write(Chunk::pack(d, 0), line);
            chunk.write(Chunk::pack(total, call_base), line);
        }
        InstKind::GetProperty { object, name } => {
            if name.as_ref() == varn_core::MemberKey::Length.as_str() {
                if let Some(crate::hir::HirType::Str) = value_tys.get(object.0 as usize) {
                    chunk.emit_rr(OpCode::StrLength, d, reg[object.0 as usize], line);
                    return Ok(());
                }
                if let Some(crate::hir::HirType::Array(_)) = value_tys.get(object.0 as usize) {
                    chunk.emit_rr(OpCode::ArrayLength, d, reg[object.0 as usize], line);
                    return Ok(());
                }
            }
            let idx = chunk.add_str(name);
            if *cache_count > 255 {
                return Err(OptError::Unsupported(
                    "ssa-emit: too many inline-cache sites",
                ));
            }
            let cs = *cache_count as u8;
            *cache_count += 1;
            chunk.emit_rrc_ic(
                OpCode::GetProperty,
                d,
                reg[object.0 as usize],
                idx,
                cs,
                line,
            );
        }
        InstKind::GetFixedField { object, slot } => {
            chunk.emit_rrc(
                OpCode::GetFixedField,
                d,
                reg[object.0 as usize],
                *slot,
                line,
            );
        }
        InstKind::GetIndex { object, index } => {
            chunk.emit_rrr(
                OpCode::GetIndex,
                d,
                reg[object.0 as usize],
                reg[index.0 as usize],
                line,
            );
        }
        InstKind::ArrayGetIndex { object, index } => {
            chunk.emit_rrr(
                OpCode::ArrayGetIndex,
                d,
                reg[object.0 as usize],
                reg[index.0 as usize],
                line,
            );
        }

        InstKind::MethodCall { recv, name, args } => {
            let name_idx = chunk.add_str(name);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[a.0 as usize], line);
            }
            let argc = args.len() as u8;
            if matches!(value_tys.get(recv.0 as usize), Some(crate::hir::HirType::Class(_))) {
                chunk.emit(OpCode::InvokeVirtual, line);
                chunk.write(Chunk::pack(d, reg[recv.0 as usize]), line);
                chunk.write(name_idx, line);
                chunk.write(Chunk::pack(argc, call_base), line);
            } else {
                if *cache_count > 255 {
                    return Err(OptError::Unsupported(
                        "ssa-emit: too many inline-cache sites",
                    ));
                }
                let cs = *cache_count as u8;
                *cache_count += 1;
                chunk.write(Chunk::pack_op(OpCode::CallMethod, cs), line);
                chunk.write(Chunk::pack(d, reg[recv.0 as usize]), line);
                chunk.write(name_idx, line);
                chunk.write(Chunk::pack(argc, call_base), line);
            }
        }
        InstKind::IsNull { operand } => {
            chunk.emit_rr(OpCode::IsNull, d, reg[operand.0 as usize], line);
        }
        InstKind::Cast { operand, .. } => {
            let src = reg[operand.0 as usize];
            if d != src {
                chunk.emit_rr(OpCode::Move, d, src, line);
            }
        }

        InstKind::BuildArray { elements } => {
            for (i, e) in elements.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[e.0 as usize], line);
            }
            chunk.emit(OpCode::BuildArray, line);
            chunk.write(Chunk::pack(d, call_base), line);
            chunk.write(Chunk::pack(elements.len() as u8, 0), line);
        }

        InstKind::BuildTuple { elements } => {
            for (i, e) in elements.iter().enumerate() {
                chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[e.0 as usize], line);
            }
            chunk.emit(OpCode::BuildTuple, line);
            chunk.write(Chunk::pack(d, call_base), line);
            chunk.write(Chunk::pack(elements.len() as u8, 0), line);
        }

        InstKind::BuildObject { pairs } => {
            let count = pairs.len();
            let mut is_contiguous = count > 0;
            let mut start_reg = call_base;
            if count > 0 {
                let first = reg[pairs[0].1 .0 as usize];
                for (i, (_, v)) in pairs.iter().enumerate() {
                    if reg[v.0 as usize] != first + i as u8 {
                        is_contiguous = false;
                        break;
                    }
                }
                if is_contiguous {
                    start_reg = first;
                } else {
                    for (i, (_, v)) in pairs.iter().enumerate() {
                        chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[v.0 as usize], line);
                    }
                }
            }
            let keys = pairs.iter().map(|(k, _)| k.clone()).collect();
            let shape_idx = chunk.add_shape(keys);
            chunk.emit(OpCode::BuildObjectWithShape, line);
            chunk.write(Chunk::pack(d, start_reg), line);
            chunk.write(shape_idx, line);
        }

        InstKind::BuildRecord { pairs } => {
            let count = pairs.len();
            let mut is_contiguous = count > 0;
            let mut start_reg = call_base;
            if count > 0 {
                let first = reg[pairs[0].1 .0 as usize];
                for (i, (_, v)) in pairs.iter().enumerate() {
                    if reg[v.0 as usize] != first + i as u8 {
                        is_contiguous = false;
                        break;
                    }
                }
                if is_contiguous {
                    start_reg = first;
                } else {
                    for (i, (_, v)) in pairs.iter().enumerate() {
                        chunk.emit_rr(OpCode::Move, call_base + i as u8, reg[v.0 as usize], line);
                    }
                }
            }
            let keys = pairs.iter().map(|(k, _)| k.clone()).collect();
            let shape_idx = chunk.add_shape(keys);
            chunk.emit(OpCode::BuildRecord, line);
            chunk.write(Chunk::pack(d, start_reg), line);
            chunk.write(shape_idx, line);
        }
        InstKind::ToString { operand } => {
            chunk.emit_rr(OpCode::ToString, d, reg[operand.0 as usize], line);
        }

        InstKind::MakeClosure { func, upvalues_src } => {
            let proto = crate::lower::lower_function(func, source_file.clone());
            let idx = chunk.add_constant(PoolEntry::Function(Rc::new(proto)));
            if upvalues_src.is_empty() {
                chunk.write(Chunk::pack_op(OpCode::LoadStaticFn, d), line);
                chunk.write(idx, line);
            } else {
                let uv_count = upvalues_src.len() as u8;
                chunk.emit(OpCode::MakeClosure, line);
                chunk.write(Chunk::pack(d, uv_count), line);
                chunk.write(idx, line);
                for uv_src in upvalues_src {
                    let is_local = match uv_src {
                        HirUpvalueSrc::ParentLocal(_) | HirUpvalueSrc::ParentParam(_) => 1u8,
                        HirUpvalueSrc::ParentUpvalue(_) => 0u8,
                    };
                    let index = match uv_src {
                        HirUpvalueSrc::ParentLocal(id) => var_reg(VarId::Local(*id), nparams),
                        HirUpvalueSrc::ParentParam(i) => var_reg(VarId::Param(*i), nparams),
                        HirUpvalueSrc::ParentUpvalue(idx) => *idx as u8,
                    };
                    chunk.write(Chunk::pack(is_local, index), line);
                }
            }
        }

        InstKind::IntrinsicCall {
            object,
            args,
            wire_byte,
        } if args.len() == 1
            && varn_core::intrinsic_ops::math::is_unary_math(*wire_byte)
            && matches!(
                value_tys.get(args[0].0 as usize),
                Some(crate::hir::HirType::Float)
            ) =>
        {
            // Direct form: no receiver staged, no window, no result Move.
            // `object` was already lowered (its side effects, if any, ran);
            // a unary math dispatch never reads it, so it simply stays in
            // its own register instead of being copied into a call slot.
            //
            // The float-argument requirement is semantic, not an
            // optimization: `intrinsics::math::dispatch` re-boxes an integral
            // result back to `int` when the argument was int-tagged, and the
            // direct form has no window for that path to round-trip through.
            // An int argument keeps the windowed encoding.
            let _ = object;
            chunk.write(Chunk::pack_op(OpCode::IntrinsicDirect, d), line);
            chunk.write(
                ((reg[args[0].0 as usize] as u16) << 8) | *wire_byte as u16,
                line,
            );
        }

        InstKind::IntrinsicCall {
            object,
            args,
            wire_byte,
        } => {
            chunk.emit_rr(OpCode::Move, call_base, reg[object.0 as usize], line);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(
                    OpCode::Move,
                    call_base + 1 + i as u8,
                    reg[a.0 as usize],
                    line,
                );
            }
            let arg_count = (args.len() + 1) as u16;
            chunk.write(Chunk::pack_op(OpCode::Intrinsic, call_base), line);
            chunk.write(((*wire_byte as u16) << 8) | arg_count, line);
            chunk.emit_rr(OpCode::Move, d, call_base, line);
        }

        InstKind::CallNativeOp {
            object,
            args,
            op_id,
        } => {
            chunk.emit_rr(OpCode::Move, call_base, reg[object.0 as usize], line);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(
                    OpCode::Move,
                    call_base + 1 + i as u8,
                    reg[a.0 as usize],
                    line,
                );
            }
            // arg_count includes the receiver; op-id stored as a constant
            // (full i64) so it survives `.vnc` serialization.
            let arg_count = (args.len() + 1) as u16;
            let cidx = chunk.add_int(*op_id as i64);
            chunk.write(Chunk::pack_op(OpCode::CallNativeOp, call_base), line);
            chunk.write(cidx, line);
            chunk.write(arg_count, line);
            chunk.emit_rr(OpCode::Move, d, call_base, line);
        }

        InstKind::BuildStr { parts } => {
            chunk.write(Chunk::pack_op(OpCode::BuildStr, d), line);
            chunk.write(Chunk::pack(parts.len() as u8, 0), line);
            for p in parts {
                chunk.write(Chunk::pack(reg[p.0 as usize], 0), line);
            }
        }
        InstKind::GetPropertyMaybe { object, name } => {
            let idx = chunk.add_str(name);
            chunk.emit_rrc(
                OpCode::GetPropertyMaybe,
                d,
                reg[object.0 as usize],
                idx,
                line,
            );
        }
        InstKind::ModuleSlot { object, slot } => {
            chunk.emit_rrc(
                OpCode::LoadModuleSlot,
                d,
                reg[object.0 as usize],
                *slot,
                line,
            );
        }
        InstKind::GetEnumTag { operand } => {
            chunk.emit_rr(OpCode::GetEnumTag, d, reg[operand.0 as usize], line);
        }
        InstKind::IsArray { operand } => {
            chunk.emit_rr(OpCode::IsArray, d, reg[operand.0 as usize], line);
        }

        InstKind::This => chunk.emit_rr(OpCode::Move, d, 0, line),

        InstKind::Range {
            start,
            end,
            inclusive,
        } => {
            let method = chunk.add_str(varn_core::well_known::RUNTIME_RANGE);
            let flag = if *inclusive { 1u8 } else { 0u8 };
            chunk.emit(OpCode::InvokeRuntimeStatic, line);
            chunk.write(Chunk::pack(d, 0), line);
            chunk.write(method, line);
            chunk.write(Chunk::pack(2, reg[start.0 as usize]), line);
            chunk.write(Chunk::pack(reg[end.0 as usize], flag), line);
        }
        InstKind::ObjectKeys { operand } => {
            chunk.emit_rr(OpCode::ObjectKeys, d, reg[operand.0 as usize], line);
        }
        InstKind::GetSymbol { object, is_async } => {
            let sym = if *is_async {
                RuntimeSymbol::AsyncIterator
            } else {
                RuntimeSymbol::Iterator
            };
            let idx = chunk.add_symbol(sym);
            chunk.emit_rrc(OpCode::GetSymbol, d, reg[object.0 as usize], idx, line);
        }

        InstKind::IterCall { callee, recv } => {
            chunk.emit_rr(OpCode::Move, call_base, reg[recv.0 as usize], line);
            chunk.emit(OpCode::Call, line);
            chunk.write(Chunk::pack(d, reg[callee.0 as usize]), line);
            chunk.write(Chunk::pack(1, call_base), line);
        }

        InstKind::GetSuper { name } => {
            let idx = chunk.add_str(name);
            chunk.emit_rc(OpCode::GetSuper, d, idx, line);
        }

        InstKind::SuperCall { args } => {
            let ctor_idx = chunk.add_str("constructor");
            chunk.emit_rc(OpCode::GetSuper, call_base, ctor_idx, line);
            chunk.emit_rr(OpCode::Move, call_base + 1, 0, line);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(
                    OpCode::Move,
                    call_base + 2 + i as u8,
                    reg[a.0 as usize],
                    line,
                );
            }
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::Call, line);
            chunk.write(Chunk::pack(call_base, call_base), line);
            chunk.write(Chunk::pack(total, call_base + 1), line);
            chunk.emit_rr(OpCode::Move, d, call_base + 1, line);
        }

        InstKind::SuperMethodCall { name, args } => {
            let name_idx = chunk.add_str(name);
            chunk.emit_rc(OpCode::GetSuper, call_base, name_idx, line);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(
                    OpCode::Move,
                    call_base + 1 + i as u8,
                    reg[a.0 as usize],
                    line,
                );
            }
            let count = args.len() as u8;
            chunk.emit(OpCode::Call, line);
            chunk.write(Chunk::pack(d, call_base), line);
            chunk.write(
                Chunk::pack(count, if count > 0 { call_base + 1 } else { 0 }),
                line,
            );
        }

        InstKind::ExtensionCall { func, recv, args } => {
            let idx = chunk.add_str(func);
            chunk.emit_rc(OpCode::LoadGlobal, call_base, idx, line);
            chunk.emit_rr(OpCode::Move, call_base + 1, reg[recv.0 as usize], line);
            for (i, a) in args.iter().enumerate() {
                chunk.emit_rr(
                    OpCode::Move,
                    call_base + 2 + i as u8,
                    reg[a.0 as usize],
                    line,
                );
            }
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::Call, line);
            chunk.write(Chunk::pack(d, call_base), line);
            chunk.write(Chunk::pack(total, call_base + 1), line);
        }

        InstKind::CallSpread { callee, args } => {
            chunk.emit_rr(OpCode::LoadNull, call_base, 0, line);
            for (i, (a, spread)) in args.iter().enumerate() {
                let op = if *spread {
                    OpCode::WrapSpread
                } else {
                    OpCode::Move
                };
                chunk.emit_rr(op, call_base + 1 + i as u8, reg[a.0 as usize], line);
            }
            let total = (args.len() + 1) as u8;
            chunk.emit(OpCode::CallSpread, line);
            chunk.write(Chunk::pack(d, reg[callee.0 as usize]), line);
            chunk.write(Chunk::pack(total, call_base), line);
        }

        InstKind::BuildArraySpread { elements } => {
            chunk.emit(OpCode::BuildArray, line);
            chunk.write(Chunk::pack(d, call_base), line);
            chunk.write(Chunk::pack(0, 0), line);
            for (v, spread) in elements {
                let op = if *spread {
                    OpCode::ArrayExtend
                } else {
                    OpCode::ArrayPush
                };
                chunk.emit_rr(op, d, reg[v.0 as usize], line);
            }
        }

        InstKind::BuildObjectSpread { parts } => {
            chunk.emit(OpCode::BuildObject, line);
            chunk.write(Chunk::pack(d, 0), line);
            for (key, v) in parts {
                match key {
                    Some(k) => {
                        let idx = chunk.add_str(k);
                        if *cache_count > 255 {
                            return Err(OptError::Unsupported(
                                "ssa-emit: too many inline-cache sites",
                            ));
                        }
                        let cs = *cache_count as u8;
                        *cache_count += 1;
                        chunk.emit_rrc_ic(OpCode::SetProperty, d, reg[v.0 as usize], idx, cs, line);
                    }
                    None => chunk.emit_rr(OpCode::ObjectMerge, d, reg[v.0 as usize], line),
                }
            }
        }
        InstKind::ObjectRest { object, skip_keys } => {
            chunk.emit(OpCode::ObjectRest, line);
            chunk.write(Chunk::pack(d, reg[object.0 as usize]), line);
            chunk.write(Chunk::pack(skip_keys.len() as u8, 0), line);
            for k in skip_keys {
                let idx = chunk.add_str(k);
                chunk.write(idx, line);
            }
        }
        InstKind::LoadCaptured { var } => {
            let src = var_reg(*var, nparams);
            chunk.emit_rr(OpCode::Move, d, src, line);
        }
        InstKind::MakeClass { name, super_class } => {
            let name_idx = chunk.add_str(name);
            let super_reg = super_class.map(|sc| reg[sc.0 as usize]).unwrap_or(0);
            chunk.emit_rrc(OpCode::MakeClass, d, super_reg, name_idx, line);
        }
        InstKind::MakeEnumVariant { tag, meta } => {
            let meta_idx = chunk.add_str(meta);
            chunk.emit_load_int(scratch, *tag, line);
            chunk.emit(OpCode::MakeEnumVariant, line);
            chunk.write(Chunk::pack(d, scratch), line);
            chunk.write(meta_idx, line);
        }
        InstKind::Try { handler } => {
            chunk.emit(OpCode::Try, line);
            chunk.write(Chunk::pack(d, 0), line);
            let pos = chunk.code.len();
            chunk.write(0xFFFF, line);
            chunk.write(0xFFFF, line);
            fixups.push((pos, *handler));
        }
        InstKind::CatchParam { try_val } => {
            chunk.emit_rr(OpCode::Move, d, reg[try_val.0 as usize], line);
        }
        InstKind::LoadModule { source } => {
            let src_idx = chunk.add_str(source);
            chunk.emit_rc(OpCode::LoadModule, d, src_idx, line);
        }
        InstKind::Await { operand } => {
            chunk.emit_rr(OpCode::Await, d, reg[operand.0 as usize], line);
        }
        InstKind::Spawn { operand } => {
            // Same 2-word shape as Await: dest in the opcode word, task
            // register in the operand word (varn_types::bytecode).
            chunk.emit_rr(OpCode::Spawn, d, reg[operand.0 as usize], line);
        }
        InstKind::Yield { operand } => {
            chunk.emit1(OpCode::Yield, Chunk::pack(d, reg[operand.0 as usize]), line);
        }

        InstKind::SetProperty { .. }
        | InstKind::SetFixedField { .. }
        | InstKind::SetIndex { .. }
        | InstKind::ArrayPush { .. }
        | InstKind::ArraySetIndex { .. }
        | InstKind::ObjectMerge { .. }
        | InstKind::AssertNotNull { .. }
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreUpvalue { .. }
        | InstKind::StoreCaptured { .. }
        | InstKind::StoreModuleSlot { .. }
        | InstKind::CloseUpvalues { .. }
        | InstKind::Dispose { .. }
        | InstKind::PopTry
        | InstKind::DeclareField { .. }
        | InstKind::DefineStatic { .. }
        | InstKind::DefineMethod { .. }
        | InstKind::DefineAccessor { .. } => {
            unreachable!()
        }
    }
    Ok(())
}
