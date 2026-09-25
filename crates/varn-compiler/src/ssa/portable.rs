//! Project the compiler's internal SSA onto the portable, serializable
//! [`varn_types::ssa::SsaProto`] that rides inside a `FunctionProto`.
//!
//! This is a *lossy by design* projection: it carries only the scalar/arith
//! family the JIT can lower directly, and returns `None` for a body that uses
//! anything else. A `None` is not an error — it is the fallback contract: the
//! function keeps its bytecode lowering, which is the pre-existing path. The
//! projection is deliberately conservative because a wrong SSA (a value whose
//! class disagrees with the bytecode) would be a miscompile, while a missing
//! SSA is only a missed optimization (Ley 10: no gain without correctness).

use std::sync::Arc;

use varn_core::OpCode;

use crate::hir::{HirBinOp, HirType, HirUnOp};
use crate::ssa::ir::{Inst, InstKind, SsaFunc, Terminator};
use varn_types::ssa::{SsaBinOp, SsaBlock, SsaInst, SsaOp, SsaProto, SsaTerm, SsaUnOp, SsaValue};

/// Build the portable SSA for `ssa` (already phi-split and register-assigned).
/// Returns why not when any instruction or terminator is outside the
/// projected family.
pub(crate) fn project(
    ssa: &SsaFunc,
    reg: &[u8],
    register_count: u16,
    has_this: bool,
    name: &Arc<str>,
    ic: &crate::ssa::ic::IcSlots,
) -> Result<SsaProto, String> {
    let value_tys: Vec<HirType> = ssa.values.iter().map(|v| v.ty).collect();

    // Module-relative global slot each value was loaded from, so a `Call` can
    // tell the linker what to resolve. Only `LoadGlobalIdx` has this provenance;
    // a callee reached any other way stays `None` and the JIT declines it.
    let mut global_of: Vec<Option<u32>> = vec![None; ssa.values.len()];
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let (Some(d), InstKind::LoadGlobalIdx(slot)) = (inst.dest, &inst.kind) {
                global_of[d.0 as usize] = Some(*slot);
            }
        }
    }

    let mut blocks = Vec::with_capacity(ssa.blocks.len());
    for (b, block) in ssa.blocks.iter().enumerate() {
        let mut insts = Vec::with_capacity(block.insts.len());
        for (i, inst) in block.insts.iter().enumerate() {
            let projected = project_inst(inst, &value_tys, &global_of, ic.of(b, i))
                .ok_or_else(|| why_not(&inst.kind, &value_tys))?;
            insts.push(projected);
        }
        blocks.push(SsaBlock {
            params: block.params.iter().map(|v| v.0).collect(),
            insts,
            term: project_term(&block.term),
        });
    }

    let values = ssa
        .values
        .iter()
        .map(|v| SsaValue {
            ty: super::emit::slot_kind_of(v.ty),
        })
        .collect();

    Ok(SsaProto {
        name: name.as_ref().into(),
        nparams: ssa
            .blocks
            .get(ssa.entry.0 as usize)
            .map(|b| b.params.len() as u32)
            .unwrap_or(0),
        entry: ssa.entry.0,
        blocks,
        values,
        regs: reg.iter().map(|&r| r as u32).collect(),
        register_count,
        has_this,
    })
}

fn project_inst(
    inst: &Inst,
    value_tys: &[HirType],
    global_of: &[Option<u32>],
    // The inline-cache slot the bytecode baked for this site (`ssa::ic`).
    ic_slot: Option<u8>,
) -> Option<SsaInst> {
    let op = match &inst.kind {
        InstKind::ConstInt(n) => SsaOp::ConstInt(*n),
        InstKind::ConstFloat(f) => SsaOp::ConstFloat(*f),
        InstKind::ConstBool(b) => SsaOp::ConstBool(*b),
        InstKind::ConstNull => SsaOp::ConstNull,
        InstKind::ConstStr(s) => SsaOp::ConstStr(s.as_ref().into()),
        InstKind::LoadGlobalIdx(slot) => SsaOp::LoadGlobalIdx(*slot),
        InstKind::Call { callee, args } => SsaOp::Call {
            callee: callee.0,
            callee_global: global_of.get(callee.0 as usize).copied().flatten(),
            args: args.iter().map(|v| v.0).collect(),
        },
        InstKind::Binary { op, lhs, rhs, ty } => {
            let lhs_ty = value_tys.get(lhs.0 as usize).copied();
            let rhs_ty = value_tys.get(rhs.0 as usize).copied();
            SsaOp::Binary {
                op: project_bin(*op, lhs_ty, rhs_ty, *ty)?,
                lhs: lhs.0,
                rhs: rhs.0,
            }
        }
        InstKind::Unary {
            op: HirUnOp::Typeof,
            operand,
            ..
        } => SsaOp::Typeof {
            operand: operand.0,
        },
        InstKind::Unary { op, operand, ty } => SsaOp::Unary {
            op: project_un(*op, *ty)?,
            operand: operand.0,
        },
        InstKind::IsArray { operand } => SsaOp::IsArray {
            operand: operand.0,
        },
        InstKind::ToString { operand } => SsaOp::ToString {
            operand: operand.0,
        },
        InstKind::ObjectKeys { operand } => SsaOp::ObjectKeys {
            operand: operand.0,
        },
        InstKind::GetEnumTag { operand } => SsaOp::GetEnumTag {
            operand: operand.0,
        },
        InstKind::BuildStr { parts } => SsaOp::BuildStr {
            parts: parts.iter().map(|v| v.0).collect(),
        },
        InstKind::BuildArray { elements } => SsaOp::BuildArray {
            elements: elements.iter().map(|v| v.0).collect(),
        },
        InstKind::BuildMap { pairs } => SsaOp::BuildMap {
            pairs: pairs.iter().map(|(k, v)| (k.0, v.0)).collect(),
        },
        InstKind::BuildObject { pairs } | InstKind::BuildRecord { pairs } => SsaOp::BuildObject {
            keys: pairs.iter().map(|(k, _)| k.as_ref().into()).collect(),
            values: pairs.iter().map(|(_, v)| v.0).collect(),
            is_record: matches!(&inst.kind, InstKind::BuildRecord { .. }),
        },
        InstKind::GetIndex { object, index }
        | InstKind::ArrayGetIndex { object, index }
        | InstKind::MapGetIndex { object, index } => SsaOp::GetIndex {
            object: object.0,
            index: index.0,
        },
        InstKind::SetIndex {
            object,
            index,
            value,
        }
        | InstKind::ArraySetIndex {
            object,
            index,
            value,
        }
        | InstKind::MapSetIndex {
            object,
            index,
            value,
        } => SsaOp::SetIndex {
            object: object.0,
            index: index.0,
            value: value.0,
        },
        InstKind::ArrayPush { array, value } => SsaOp::ArrayPush {
            array: array.0,
            value: value.0,
        },
        InstKind::This => SsaOp::This,
        InstKind::GetFixedField {
            object,
            slot,
            offset,
            tag,
        } => SsaOp::GetFixedField {
            object: object.0,
            slot: *slot,
            offset: *offset,
            access: *tag,
        },
        InstKind::SetFixedField {
            object,
            value,
            slot,
            offset,
            tag,
        } => SsaOp::SetFixedField {
            object: object.0,
            value: value.0,
            slot: *slot,
            offset: *offset,
            kind: *tag,
        },
        InstKind::MakeClass { name, super_class } => SsaOp::MakeClass {
            name: name.as_ref().into(),
            super_class: super_class.map(|v| v.0),
        },
        InstKind::DeclareField { class, name, tag } => SsaOp::DeclareField {
            class: class.0,
            name: name.as_ref().into(),
            tag: *tag,
        },
        InstKind::DefineStatic { class, name, value } => SsaOp::DefineMethod {
            class: class.0,
            name: name.as_ref().into(),
            member: value.0,
            kind: 1,
        },
        InstKind::DefineMethod {
            class,
            name,
            method,
            is_static,
        } => SsaOp::DefineMethod {
            class: class.0,
            name: name.as_ref().into(),
            member: method.0,
            kind: if *is_static { 1 } else { 0 },
        },
        InstKind::DefineAccessor {
            class,
            name,
            accessor,
            is_getter,
            is_static,
        } => SsaOp::DefineMethod {
            class: class.0,
            name: name.as_ref().into(),
            member: accessor.0,
            kind: 2 + if *is_static { 2 } else { 0 } + if *is_getter { 0 } else { 1 },
        },
        InstKind::GetSuper { name } => SsaOp::GetSuper {
            name: name.as_ref().into(),
        },
        InstKind::ArrayLength { operand } => SsaOp::ArrayLength { operand: operand.0 },
        InstKind::StrLength { operand } => SsaOp::StrLength { operand: operand.0 },
        InstKind::GetProperty { object, name } => SsaOp::GetProperty {
            object: object.0,
            name: name.as_ref().into(),
            cs: u16::from(ic_slot?),
        },
        InstKind::SetProperty {
            object,
            name,
            value,
        } => SsaOp::SetProperty {
            object: object.0,
            value: value.0,
            name: name.as_ref().into(),
            cs: u16::from(ic_slot?),
        },
        InstKind::SelfCall { args } => SsaOp::SelfCall {
            args: args.iter().map(|v| v.0).collect(),
        },
        InstKind::Cast { operand, .. } => SsaOp::Cast {
            operand: operand.0,
        },
        InstKind::Convert { operand, conv } => SsaOp::Convert {
            operand: operand.0,
            conv: *conv,
        },
        InstKind::IsNull { operand } => SsaOp::IsNull {
            operand: operand.0,
        },
        // Calls, heap ops, closures, classes, suspension: outside the family.
        _ => return None,
    };
    Some(SsaInst {
        dest: inst.dest.map(|v| v.0),
        op,
        line: inst.line,
    })
}

fn project_term(term: &Terminator) -> SsaTerm {
    match term {
        Terminator::Return(v) => SsaTerm::Return(v.map(|v| v.0)),
        Terminator::Throw(v) => SsaTerm::Throw(v.0),
        Terminator::Jump { target, args } => SsaTerm::Jump {
            target: target.0,
            args: args.iter().map(|v| v.0).collect(),
        },
        Terminator::Branch {
            cond,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } => SsaTerm::Branch {
            cond: cond.0,
            then_blk: then_blk.0,
            then_args: then_args.iter().map(|v| v.0).collect(),
            else_blk: else_blk.0,
            else_args: else_args.iter().map(|v| v.0).collect(),
        },
        Terminator::Unreachable => SsaTerm::Unreachable,
    }
}

/// Why an instruction has no portable form: its name, and for an operator
/// the operand types it was asked for.
fn why_not(kind: &InstKind, value_tys: &[HirType]) -> String {
    let ty = |v: &crate::ssa::ir::Value| value_tys.get(v.0 as usize).copied();
    match kind {
        InstKind::Binary { op, lhs, rhs, .. } => {
            format!("no portable form for {op:?} on {:?} and {:?}", ty(lhs), ty(rhs))
        }
        InstKind::Unary { op, operand, .. } => {
            format!("no portable form for {op:?} on {:?}", ty(operand))
        }
        other => {
            let full = format!("{other:?}");
            let end = full.find([' ', '(', '{']).unwrap_or(full.len());
            format!("no portable form for {}", &full[..end])
        }
    }
}

/// The typed `OpCode` the emitter already selects, mapped to its portable
/// mirror. Generic (dynamic-operand) arithmetic has no portable variant, so it
/// declines.
fn project_bin(
    op: HirBinOp,
    lhs_ty: Option<HirType>,
    rhs_ty: Option<HirType>,
    ty: HirType,
) -> Option<SsaBinOp> {
    let str_operand = matches!(op, HirBinOp::Add)
        && (matches!(lhs_ty, Some(HirType::Str)) || matches!(rhs_ty, Some(HirType::Str)));
    let opcode = if str_operand {
        OpCode::StrConcat
    } else {
        crate::lower::bin_opcode(op, ty)
    };
    Some(match opcode {
        OpCode::AddInt => SsaBinOp::IntAdd,
        OpCode::SubInt => SsaBinOp::IntSub,
        OpCode::MulInt => SsaBinOp::IntMul,
        OpCode::DivInt => SsaBinOp::IntDiv,
        OpCode::ModInt => SsaBinOp::IntMod,
        OpCode::PowInt => SsaBinOp::IntPow,
        OpCode::EqInt => SsaBinOp::IntEq,
        OpCode::NeqInt => SsaBinOp::IntNe,
        OpCode::LtInt => SsaBinOp::IntLt,
        OpCode::LteInt => SsaBinOp::IntLe,
        OpCode::GtInt => SsaBinOp::IntGt,
        OpCode::GteInt => SsaBinOp::IntGe,
        OpCode::BitAnd => SsaBinOp::IntAnd,
        OpCode::BitOr => SsaBinOp::IntOr,
        OpCode::BitXor => SsaBinOp::IntXor,
        OpCode::Shl => SsaBinOp::IntShl,
        OpCode::Shr => SsaBinOp::IntShr,
        OpCode::Ushr => SsaBinOp::IntUshr,
        OpCode::AddFloat => SsaBinOp::FloatAdd,
        OpCode::SubFloat => SsaBinOp::FloatSub,
        OpCode::MulFloat => SsaBinOp::FloatMul,
        OpCode::DivFloat => SsaBinOp::FloatDiv,
        OpCode::ModFloat => SsaBinOp::FloatMod,
        OpCode::PowFloat => SsaBinOp::FloatPow,
        OpCode::EqFloat => SsaBinOp::FloatEq,
        OpCode::NeqFloat => SsaBinOp::FloatNe,
        OpCode::LtFloat => SsaBinOp::FloatLt,
        OpCode::LteFloat => SsaBinOp::FloatLe,
        OpCode::GtFloat => SsaBinOp::FloatGt,
        OpCode::GteFloat => SsaBinOp::FloatGe,
        OpCode::StrConcat => SsaBinOp::StrConcat,
        _ => return None,
    })
}

fn project_un(op: HirUnOp, ty: HirType) -> Option<SsaUnOp> {
    Some(match op {
        HirUnOp::Neg => match ty {
            HirType::Int => SsaUnOp::NegInt,
            HirType::Float => SsaUnOp::NegFloat,
            _ => return None,
        },
        HirUnOp::Not => SsaUnOp::Not,
        HirUnOp::BitNot => SsaUnOp::BitNotInt,
        HirUnOp::Typeof => return None,
    })
}
