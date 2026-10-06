use super::super::{Captured, Site};
use crate::hir::HirType;
use crate::ssa::ir::InstKind;
use varn_types::ssa::SsaOp;

pub(super) fn try_project(
    kind: &InstKind,
    value_tys: &[HirType],
    site: &Site,
    captured: &mut Captured,
) -> Option<SsaOp> {
    match kind {
        InstKind::Binary { op, lhs, rhs, ty } => {
            let lhs_ty = value_tys.get(lhs.0 as usize).copied();
            let rhs_ty = value_tys.get(rhs.0 as usize).copied();
            Some(SsaOp::Binary {
                op: super::super::ops::project_bin(*op, lhs_ty, rhs_ty, *ty)?,
                lhs: lhs.0,
                rhs: rhs.0,
            })
        }
        InstKind::Unary {
            op: crate::hir::HirUnOp::Typeof,
            operand,
            ..
        } => Some(SsaOp::Typeof { operand: operand.0 }),
        InstKind::Unary { op, operand, .. } => Some(SsaOp::Unary {
            op: super::super::ops::project_un(*op, value_tys.get(operand.0 as usize).copied())?,
            operand: operand.0,
        }),
        InstKind::IsArray { operand } => Some(SsaOp::IsArray { operand: operand.0 }),
        InstKind::ToString { operand } => Some(SsaOp::ToString { operand: operand.0 }),
        InstKind::ObjectKeys { operand } => Some(SsaOp::ObjectKeys { operand: operand.0 }),
        InstKind::GetEnumTag { operand } => Some(SsaOp::GetEnumTag { operand: operand.0 }),
        InstKind::Cast { operand, .. } => Some(SsaOp::Cast { operand: operand.0 }),
        InstKind::Convert { operand, conv } => Some(SsaOp::Convert {
            operand: operand.0,
            conv: *conv,
        }),
        InstKind::IsNull { operand } => Some(SsaOp::IsNull { operand: operand.0 }),
        InstKind::AssertNotNull { operand } => Some(SsaOp::AssertNotNull { operand: operand.0 }),
        InstKind::Range {
            start,
            end,
            inclusive,
        } => Some(SsaOp::Range {
            start: start.0,
            end: end.0,
            inclusive: *inclusive,
        }),
        InstKind::PopTry => Some(SsaOp::PopTry),
        InstKind::CatchParam { try_val } => Some(SsaOp::CatchParam { try_val: try_val.0 }),
        InstKind::Await { operand } => Some(SsaOp::Await {
            operand: operand.0,
            resume_ip: site.next_ip,
            live: site.resume_live.clone(),
        }),
        InstKind::Spawn { operand } => Some(SsaOp::Spawn { operand: operand.0 }),
        InstKind::Yield { operand } => Some(SsaOp::Yield {
            operand: operand.0,
            resume_ip: site.next_ip,
            live: site.resume_live.clone(),
        }),
        InstKind::Dispose { target, is_await } => Some(SsaOp::Dispose {
            var: captured.index(crate::ssa::ir::VarId::Local(*target)),
            is_await: *is_await,
            cs: u16::from(site.ic_slot?),
        }),
        _ => None,
    }
}
