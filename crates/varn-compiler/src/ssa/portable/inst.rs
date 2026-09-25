//! One instruction of the compiler's SSA, projected onto its portable form.

use crate::hir::{HirType, HirUnOp, HirUpvalueSrc};
use crate::ssa::ir::{Inst, InstKind, VarId};
use varn_types::ssa::{SsaInst, SsaOp, SsaUpvalue};

use super::ops::{project_bin, project_un};
use super::{Captured, Site};

pub(super) fn project_inst(
    inst: &Inst,
    value_tys: &[HirType],
    global_of: &[Option<u32>],
    site: Site,
    captured: &mut Captured,
) -> Option<SsaInst> {
    let ic_slot = site.ic_slot;
    let op = match &inst.kind {
        InstKind::ConstInt(n) => SsaOp::ConstInt(*n),
        InstKind::ConstFloat(f) => SsaOp::ConstFloat(*f),
        InstKind::ConstBool(b) => SsaOp::ConstBool(*b),
        InstKind::ConstNull => SsaOp::ConstNull,
        InstKind::ConstStr(s) => SsaOp::ConstStr(s.as_ref().into()),
        InstKind::ConstChar(c) => SsaOp::ConstChar(*c),
        InstKind::ConstBigInt(digits) => SsaOp::ConstBigInt(digits.as_ref().into()),
        InstKind::ConstDecimal(d) => SsaOp::ConstDecimal(d.to_string().into()),
        InstKind::MakeEnumVariant { tag, meta } => SsaOp::MakeEnumVariant {
            tag: *tag,
            meta: meta.as_ref().into(),
        },
        InstKind::IntrinsicCall {
            object,
            args,
            wire_byte,
        } => SsaOp::IntrinsicCall {
            object: object.0,
            args: args.iter().map(|v| v.0).collect(),
            wire: *wire_byte,
        },
        InstKind::LoadGlobalIdx(slot) => SsaOp::LoadGlobalIdx(*slot),
        InstKind::LoadNativeGlobalIdx(slot) => SsaOp::LoadNativeGlobalIdx(*slot),
        InstKind::StoreGlobalIdx { slot, value } => SsaOp::StoreGlobalIdx {
            slot: *slot,
            value: value.0,
        },
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
        } => SsaOp::Typeof { operand: operand.0 },
        InstKind::Unary { op, operand, .. } => SsaOp::Unary {
            op: project_un(*op, value_tys.get(operand.0 as usize).copied())?,
            operand: operand.0,
        },
        InstKind::IsArray { operand } => SsaOp::IsArray { operand: operand.0 },
        InstKind::ToString { operand } => SsaOp::ToString { operand: operand.0 },
        InstKind::ObjectKeys { operand } => SsaOp::ObjectKeys { operand: operand.0 },
        InstKind::GetEnumTag { operand } => SsaOp::GetEnumTag { operand: operand.0 },
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
        InstKind::ArrayGetIndex { object, index } if is_int(value_tys, *index) => {
            SsaOp::ArrayGetIndex {
                object: object.0,
                index: index.0,
            }
        }
        InstKind::ArraySetIndex {
            object,
            index,
            value,
        } if is_int(value_tys, *index) => SsaOp::ArraySetIndex {
            object: object.0,
            index: index.0,
            value: value.0,
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
        InstKind::MethodCall { recv, name, args } => SsaOp::MethodCall {
            recv: recv.0,
            name: name.as_ref().into(),
            args: args.iter().map(|v| v.0).collect(),
            cs: u16::from(ic_slot?),
        },
        InstKind::CallNativeOp {
            object,
            args,
            op_id,
        } => SsaOp::CallNativeOp {
            object: object.0,
            args: args.iter().map(|v| v.0).collect(),
            op_id: *op_id,
        },
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
        InstKind::Cast { operand, .. } => SsaOp::Cast { operand: operand.0 },
        InstKind::Convert { operand, conv } => SsaOp::Convert {
            operand: operand.0,
            conv: *conv,
        },
        InstKind::IsNull { operand } => SsaOp::IsNull { operand: operand.0 },
        InstKind::MakeClosure { upvalues_src, .. } => SsaOp::MakeClosure {
            proto: u32::from(site.closure_const?),
            upvalues: upvalues_src
                .iter()
                .map(|src| match src {
                    HirUpvalueSrc::ParentLocal(id) => {
                        SsaUpvalue::Captured(captured.index(VarId::Local(*id)))
                    }
                    HirUpvalueSrc::ParentParam(i) => {
                        SsaUpvalue::Captured(captured.index(VarId::Param(*i)))
                    }
                    HirUpvalueSrc::ParentUpvalue(idx) => SsaUpvalue::Inherited(*idx),
                })
                .collect(),
        },
        InstKind::LoadCaptured { var } => SsaOp::LoadCaptured {
            var: captured.index(*var),
        },
        InstKind::StoreCaptured { var, value } => SsaOp::StoreCaptured {
            var: captured.index(*var),
            value: value.0,
        },
        InstKind::LoadUpvalue(index) => SsaOp::LoadUpvalue(*index),
        InstKind::StoreUpvalue { index, value } => SsaOp::StoreUpvalue {
            index: *index,
            value: value.0,
        },
        InstKind::CloseUpvalues { targets } => SsaOp::CloseUpvalues {
            vars: targets.iter().map(|t| captured.index(*t)).collect(),
        },
        // The landing pad writes the thrown value to the `Try`'s value at run
        // time; the op itself defines nothing.
        InstKind::Try { .. } => {
            let (catch_ip, live) = site.landing?;
            let mut live: Vec<u32> = live.iter().copied().collect();
            live.sort_unstable();
            return Some(SsaInst {
                dest: None,
                op: SsaOp::Try {
                    catch_ip,
                    catch_value: inst.dest?.0,
                    live,
                },
                line: inst.line,
            });
        }
        InstKind::PopTry => SsaOp::PopTry,
        InstKind::CatchParam { try_val } => SsaOp::CatchParam { try_val: try_val.0 },
        // Calls, heap ops, closures, classes, suspension: outside the family.
        _ => return None,
    };
    Some(SsaInst {
        dest: inst.dest.map(|v| v.0),
        op,
        line: inst.line,
    })
}

/// Whether `v` is a native `int` in the portable SSA.
fn is_int(value_tys: &[HirType], v: crate::ssa::ir::Value) -> bool {
    value_tys.get(v.0 as usize).is_some_and(|t| {
        crate::ssa::emit::slot_kind_of(*t) == varn_types::register_meta::SlotKind::Int
    })
}
