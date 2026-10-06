use crate::ty::BackendTy;
use crate::TirModule;

pub(super) fn assignable(m: &TirModule, from: BackendTy, to: BackendTy) -> bool {
    assignable_with_depth(m, from, to, 0)
}

const ASSIGNABLE_DEPTH_LIMIT: usize = 32;

fn assignable_with_depth(m: &TirModule, from: BackendTy, to: BackendTy, depth: usize) -> bool {
    if depth > ASSIGNABLE_DEPTH_LIMIT {
        return true;
    }

    if matches!(from, BackendTy::Dynamic(_)) || matches!(to, BackendTy::Dynamic(_)) {
        return true;
    }
    if from == to {
        return true;
    }

    if from == BackendTy::Never {
        return true;
    }

    if from == BackendTy::Int
        && matches!(
            to,
            BackendTy::Float | BackendTy::Decimal | BackendTy::BigInt
        )
    {
        return true;
    }

    match (from, to) {
        (BackendTy::Array(a), BackendTy::Array(b)) | (BackendTy::Set(a), BackendTy::Set(b))
            if m.types.contains(a) && m.types.contains(b) =>
        {
            return assignable_with_depth(m, m.types.get(a), m.types.get(b), depth + 1);
        }
        _ => {}
    }

    if let (BackendTy::Class(sub), BackendTy::Class(sup)) = (from, to) {
        let mut cur = Some(sub);
        let mut hops = 0;
        while let Some(c) = cur {
            if c == sup {
                return true;
            }
            if hops > ASSIGNABLE_DEPTH_LIMIT {
                break;
            }
            hops += 1;
            cur = m.class(c).and_then(|ci| ci.parent);
        }
    }
    if let (BackendTy::Nullable(fi), BackendTy::Nullable(ti)) = (from, to) {
        if !m.types.contains(fi) || !m.types.contains(ti) {
            return true;
        }
        return assignable_with_depth(m, m.types.get(fi), m.types.get(ti), depth + 1);
    }

    if let BackendTy::Nullable(inner) = to {
        if !m.types.contains(inner) {
            return true;
        }
        return assignable_with_depth(m, from, m.types.get(inner), depth + 1);
    }
    false
}
