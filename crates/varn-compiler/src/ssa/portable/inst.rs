use varn_tir::BackendTy;
mod calls;
mod composite;
mod composite_class;
mod composite_object;
mod consts;
mod control;

use super::super::ir::{Inst, InstKind};
use super::{Captured, Site};
use varn_types::ssa::{SsaInst, SsaOp};

pub(super) fn project_inst(
    inst: &Inst,
    value_tys: &[BackendTy],
    site: Site,
    captured: &mut Captured,
) -> Option<SsaInst> {
    if let InstKind::Try { .. } = &inst.kind {
        let (catch_ip, live) = site.landing?;
        let catch_value = inst.dest?.0;
        let mut live: Vec<u32> = live.iter().copied().filter(|v| *v != catch_value).collect();
        live.sort_unstable();
        return Some(SsaInst {
            dest: None,
            op: SsaOp::Try {
                catch_ip,
                catch_value,
                live,
            },
            line: inst.line,
        });
    }
    let kind = &inst.kind;
    let op = consts::try_project(kind, &site)
        .or_else(|| calls::try_project(kind, &site, captured))
        .or_else(|| composite::try_project(kind, value_tys, &site))
        .or_else(|| control::try_project(kind, value_tys, &site, captured))?;
    Some(SsaInst {
        dest: inst.dest.map(|v| v.0),
        op,
        line: inst.line,
    })
}
