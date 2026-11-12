use std::collections::BTreeMap;

use rustc_hash::{FxHashMap, FxHashSet};
use vn_types::ssa::SsaProto;

pub(super) fn global_sources(ssa: &SsaProto) -> BTreeMap<u32, u32> {
    let mut global_src: BTreeMap<u32, u32> = Default::default();
    for blk in &ssa.blocks {
        for inst in &blk.insts {
            if let (Some(d), vn_types::ssa::SsaOp::LoadGlobalIdx(slot)) = (inst.dest, &inst.op) {
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
                    vn_types::ssa::SsaOp::LoadGlobalIdx(slot) => {
                        if !loaded.contains(slot) {
                            loaded.push(*slot);
                        }
                    }
                    vn_types::ssa::SsaOp::StoreGlobalIdx { slot, .. } => {
                        stored.insert(*slot);
                    }
                    op @ (vn_types::ssa::SsaOp::ConstInt(_)
                    | vn_types::ssa::SsaOp::ConstFloat(_)
                    | vn_types::ssa::SsaOp::ConstBool(_)
                    | vn_types::ssa::SsaOp::ConstNull
                    | vn_types::ssa::SsaOp::ConstStr(_)
                    | vn_types::ssa::SsaOp::Binary { .. }
                    | vn_types::ssa::SsaOp::Unary { .. }
                    | vn_types::ssa::SsaOp::SelfCall { .. }
                    | vn_types::ssa::SsaOp::Call { .. }
                    | vn_types::ssa::SsaOp::AllocInstance { .. }
                    | vn_types::ssa::SsaOp::ArrayGetIndex { .. }
                    | vn_types::ssa::SsaOp::BytesGetIndex { .. }
                    | vn_types::ssa::SsaOp::ArraySetIndex { .. }
                    | vn_types::ssa::SsaOp::BytesSetIndex { .. }
                    | vn_types::ssa::SsaOp::ConstChar(_)
                    | vn_types::ssa::SsaOp::ConstBigInt(_)
                    | vn_types::ssa::SsaOp::ConstDecimal(_)
                    | vn_types::ssa::SsaOp::MakeEnumVariant { .. }
                    | vn_types::ssa::SsaOp::IntrinsicCall { .. }
                    | vn_types::ssa::SsaOp::LoadNativeGlobalIdx(_)
                    | vn_types::ssa::SsaOp::Try { .. }
                    | vn_types::ssa::SsaOp::PopTry
                    | vn_types::ssa::SsaOp::CatchParam { .. }
                    | vn_types::ssa::SsaOp::MakeClosure { .. }
                    | vn_types::ssa::SsaOp::LoadCaptured { .. }
                    | vn_types::ssa::SsaOp::StoreCaptured { .. }
                    | vn_types::ssa::SsaOp::LoadUpvalue(_)
                    | vn_types::ssa::SsaOp::StoreUpvalue { .. }
                    | vn_types::ssa::SsaOp::CloseUpvalues { .. }
                    | vn_types::ssa::SsaOp::Cast { .. }
                    | vn_types::ssa::SsaOp::Convert { .. }
                    | vn_types::ssa::SsaOp::IsNull { .. }
                    | vn_types::ssa::SsaOp::Typeof { .. }
                    | vn_types::ssa::SsaOp::ToString { .. }
                    | vn_types::ssa::SsaOp::IsArray { .. }
                    | vn_types::ssa::SsaOp::GetEnumTag { .. }
                    | vn_types::ssa::SsaOp::ObjectKeys { .. }
                    | vn_types::ssa::SsaOp::BuildStr { .. }
                    | vn_types::ssa::SsaOp::BuildArray { .. }
                    | vn_types::ssa::SsaOp::BuildMap { .. }
                    | vn_types::ssa::SsaOp::BuildObject { .. }
                    | vn_types::ssa::SsaOp::GetProperty { .. }
                    | vn_types::ssa::SsaOp::SetProperty { .. }
                    | vn_types::ssa::SsaOp::GetIndex { .. }
                    | vn_types::ssa::SsaOp::SetIndex { .. }
                    | vn_types::ssa::SsaOp::ArrayLength { .. }
                    | vn_types::ssa::SsaOp::StrLength { .. }
                    | vn_types::ssa::SsaOp::BytesLength { .. }
                    | vn_types::ssa::SsaOp::MethodCall { .. }
                    | vn_types::ssa::SsaOp::CallNativeOp { .. }
                    | vn_types::ssa::SsaOp::ArrayPush { .. }
                    | vn_types::ssa::SsaOp::This
                    | vn_types::ssa::SsaOp::GetFixedField { .. }
                    | vn_types::ssa::SsaOp::SetFixedField { .. }
                    | vn_types::ssa::SsaOp::MakeClass { .. }
                    | vn_types::ssa::SsaOp::DeclareLayout { .. }
                    | vn_types::ssa::SsaOp::DefineMethod { .. }
                    | vn_types::ssa::SsaOp::GetSuper { .. }
                    | vn_types::ssa::SsaOp::LoadGlobal(_)
                    | vn_types::ssa::SsaOp::StoreGlobal { .. }
                    | vn_types::ssa::SsaOp::BuildTuple { .. }
                    | vn_types::ssa::SsaOp::BuildArraySpread { .. }
                    | vn_types::ssa::SsaOp::BuildObjectSpread { .. }
                    | vn_types::ssa::SsaOp::ObjectMerge { .. }
                    | vn_types::ssa::SsaOp::ObjectRest { .. }
                    | vn_types::ssa::SsaOp::GetPropertyMaybe { .. }
                    | vn_types::ssa::SsaOp::AssertNotNull { .. }
                    | vn_types::ssa::SsaOp::BindMethod { .. }
                    | vn_types::ssa::SsaOp::ArrayExtend { .. }
                    | vn_types::ssa::SsaOp::WrapSpread { .. }
                    | vn_types::ssa::SsaOp::Range { .. }
                    | vn_types::ssa::SsaOp::SuperCall { .. }
                    | vn_types::ssa::SsaOp::SuperMethodCall { .. }
                    | vn_types::ssa::SsaOp::ExtensionCall { .. }
                    | vn_types::ssa::SsaOp::CallSpread { .. }
                    | vn_types::ssa::SsaOp::LoadModule { .. }
                    | vn_types::ssa::SsaOp::ModuleSlot { .. }
                    | vn_types::ssa::SsaOp::StoreModuleSlot { .. }
                    | vn_types::ssa::SsaOp::Await { .. }
                    | vn_types::ssa::SsaOp::Spawn { .. }
                    | vn_types::ssa::SsaOp::Yield { .. }
                    | vn_types::ssa::SsaOp::Dispose { .. }) => {
                        if !super::range_versioning::version_op_ok(ssa, op) {
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
