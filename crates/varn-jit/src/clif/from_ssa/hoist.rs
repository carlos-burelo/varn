use std::collections::BTreeMap;

use rustc_hash::{FxHashMap, FxHashSet};
use varn_types::ssa::SsaProto;

use super::ranges;

pub(super) fn global_sources(ssa: &SsaProto) -> BTreeMap<u32, u32> {
    let mut global_src: BTreeMap<u32, u32> = Default::default();
    for blk in &ssa.blocks {
        for inst in &blk.insts {
            if let (Some(d), varn_types::ssa::SsaOp::LoadGlobalIdx(slot)) = (inst.dest, &inst.op) {
                global_src.insert(d, *slot);
            }
        }
    }
    global_src
}

pub(super) fn plan_hoists(
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    reached: &[bool],
    loop_headers: &[usize],
    loop_bodies: &FxHashMap<usize, FxHashSet<usize>>,
    has_frame: bool,
) -> BTreeMap<usize, Vec<(usize, u32)>> {
    let mut hoist_at: BTreeMap<usize, Vec<(usize, u32)>> = Default::default();
    for &h in loop_headers {
        let body = &loop_bodies[&h];
        let outside: Vec<usize> = preds[h]
            .iter()
            .copied()
            .filter(|p| reached[*p] && !body.contains(p))
            .collect();
        if outside.len() != 1 || !has_frame {
            continue;
        }
        let mut stored: FxHashSet<u32> = FxHashSet::default();
        let mut loaded: Vec<u32> = Vec::new();
        let mut pure = true;
        for &b in body {
            if !reached[b] {
                continue;
            }
            for inst in &ssa.blocks[b].insts {
                match &inst.op {
                    varn_types::ssa::SsaOp::LoadGlobalIdx(slot) => {
                        if !loaded.contains(slot) {
                            loaded.push(*slot);
                        }
                    }
                    varn_types::ssa::SsaOp::StoreGlobalIdx { slot, .. } => {
                        stored.insert(*slot);
                    }
                    op @ (varn_types::ssa::SsaOp::ConstInt(_)
                    | varn_types::ssa::SsaOp::ConstFloat(_)
                    | varn_types::ssa::SsaOp::ConstBool(_)
                    | varn_types::ssa::SsaOp::ConstNull
                    | varn_types::ssa::SsaOp::ConstStr(_)
                    | varn_types::ssa::SsaOp::Binary { .. }
                    | varn_types::ssa::SsaOp::Unary { .. }
                    | varn_types::ssa::SsaOp::SelfCall { .. }
                    | varn_types::ssa::SsaOp::Call { .. }
                    | varn_types::ssa::SsaOp::AllocInstance { .. }
                    | varn_types::ssa::SsaOp::ArrayGetIndex { .. }
                    | varn_types::ssa::SsaOp::ArraySetIndex { .. }
                    | varn_types::ssa::SsaOp::ConstChar(_)
                    | varn_types::ssa::SsaOp::ConstBigInt(_)
                    | varn_types::ssa::SsaOp::ConstDecimal(_)
                    | varn_types::ssa::SsaOp::MakeEnumVariant { .. }
                    | varn_types::ssa::SsaOp::IntrinsicCall { .. }
                    | varn_types::ssa::SsaOp::LoadNativeGlobalIdx(_)
                    | varn_types::ssa::SsaOp::Try { .. }
                    | varn_types::ssa::SsaOp::PopTry
                    | varn_types::ssa::SsaOp::CatchParam { .. }
                    | varn_types::ssa::SsaOp::MakeClosure { .. }
                    | varn_types::ssa::SsaOp::LoadCaptured { .. }
                    | varn_types::ssa::SsaOp::StoreCaptured { .. }
                    | varn_types::ssa::SsaOp::LoadUpvalue(_)
                    | varn_types::ssa::SsaOp::StoreUpvalue { .. }
                    | varn_types::ssa::SsaOp::CloseUpvalues { .. }
                    | varn_types::ssa::SsaOp::Cast { .. }
                    | varn_types::ssa::SsaOp::Convert { .. }
                    | varn_types::ssa::SsaOp::IsNull { .. }
                    | varn_types::ssa::SsaOp::Typeof { .. }
                    | varn_types::ssa::SsaOp::ToString { .. }
                    | varn_types::ssa::SsaOp::IsArray { .. }
                    | varn_types::ssa::SsaOp::GetEnumTag { .. }
                    | varn_types::ssa::SsaOp::ObjectKeys { .. }
                    | varn_types::ssa::SsaOp::BuildStr { .. }
                    | varn_types::ssa::SsaOp::BuildArray { .. }
                    | varn_types::ssa::SsaOp::BuildMap { .. }
                    | varn_types::ssa::SsaOp::BuildObject { .. }
                    | varn_types::ssa::SsaOp::GetProperty { .. }
                    | varn_types::ssa::SsaOp::SetProperty { .. }
                    | varn_types::ssa::SsaOp::GetIndex { .. }
                    | varn_types::ssa::SsaOp::SetIndex { .. }
                    | varn_types::ssa::SsaOp::ArrayLength { .. }
                    | varn_types::ssa::SsaOp::StrLength { .. }
                    | varn_types::ssa::SsaOp::BytesLength { .. }
                    | varn_types::ssa::SsaOp::MethodCall { .. }
                    | varn_types::ssa::SsaOp::CallNativeOp { .. }
                    | varn_types::ssa::SsaOp::ArrayPush { .. }
                    | varn_types::ssa::SsaOp::This
                    | varn_types::ssa::SsaOp::GetFixedField { .. }
                    | varn_types::ssa::SsaOp::SetFixedField { .. }
                    | varn_types::ssa::SsaOp::MakeClass { .. }
                    | varn_types::ssa::SsaOp::DeclareLayout { .. }
                    | varn_types::ssa::SsaOp::DefineMethod { .. }
                    | varn_types::ssa::SsaOp::GetSuper { .. }
                    | varn_types::ssa::SsaOp::LoadGlobal(_)
                    | varn_types::ssa::SsaOp::StoreGlobal { .. }
                    | varn_types::ssa::SsaOp::BuildTuple { .. }
                    | varn_types::ssa::SsaOp::BuildArraySpread { .. }
                    | varn_types::ssa::SsaOp::BuildObjectSpread { .. }
                    | varn_types::ssa::SsaOp::ObjectMerge { .. }
                    | varn_types::ssa::SsaOp::ObjectRest { .. }
                    | varn_types::ssa::SsaOp::GetPropertyMaybe { .. }
                    | varn_types::ssa::SsaOp::AssertNotNull { .. }
                    | varn_types::ssa::SsaOp::BindMethod { .. }
                    | varn_types::ssa::SsaOp::ArrayExtend { .. }
                    | varn_types::ssa::SsaOp::WrapSpread { .. }
                    | varn_types::ssa::SsaOp::Range { .. }
                    | varn_types::ssa::SsaOp::GetSymbol { .. }
                    | varn_types::ssa::SsaOp::IterCall { .. }
                    | varn_types::ssa::SsaOp::SuperCall { .. }
                    | varn_types::ssa::SsaOp::SuperMethodCall { .. }
                    | varn_types::ssa::SsaOp::ExtensionCall { .. }
                    | varn_types::ssa::SsaOp::CallSpread { .. }
                    | varn_types::ssa::SsaOp::LoadModule { .. }
                    | varn_types::ssa::SsaOp::ModuleSlot { .. }
                    | varn_types::ssa::SsaOp::StoreModuleSlot { .. }
                    | varn_types::ssa::SsaOp::Await { .. }
                    | varn_types::ssa::SsaOp::Spawn { .. }
                    | varn_types::ssa::SsaOp::Yield { .. }
                    | varn_types::ssa::SsaOp::Dispose { .. }) => {
                        if !ranges::version_op_ok(ssa, op) {
                            pure = false;
                        }
                    }
                }
            }
        }
        if !pure {
            continue;
        }
        let slots: Vec<u32> = loaded.into_iter().filter(|s| !stored.contains(s)).collect();
        if !slots.is_empty() {
            hoist_at
                .entry(outside[0])
                .or_default()
                .extend(slots.into_iter().map(|s| (h, s)));
        }
    }
    hoist_at
}
