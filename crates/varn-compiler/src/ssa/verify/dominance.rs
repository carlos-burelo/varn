use super::structural::VerifyResult;
use crate::ssa::dom::DomTree;
use crate::ssa::ir::{BlockId, Value};

#[derive(Clone, Copy)]
pub(super) struct DefSite {
    pub(super) block: BlockId,
    pub(super) order: u32,
}

pub(super) fn check_use(
    def: &[Option<DefSite>],
    dom: &DomTree,
    v: Value,
    use_block: BlockId,
    use_order: u32,
) -> VerifyResult {
    let site = def
        .get(v.0 as usize)
        .copied()
        .flatten()
        .ok_or_else(|| format!("use of undefined value v{}", v.0))?;
    if site.block == use_block {
        if site.order >= use_order {
            return Err(format!("value v{} used before its definition", v.0));
        }
        return Ok(());
    }
    if dom.reachable(use_block.0 as usize)
        && dom.dominates(site.block.0 as usize, use_block.0 as usize)
    {
        Ok(())
    } else {
        Err(format!(
            "def of v{} (b{}) does not dominate use (b{})",
            v.0, site.block.0, use_block.0
        ))
    }
}
