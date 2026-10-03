use super::ids::{CheckerTyId, FunctionTypeId, InternedTypeKind, ObjectMembersId, TyListId};
use crate::types::{FunctionType, ObjectTypeMember};
use rustc_hash::FxHasher;
use std::hash::{Hash, Hasher};

/// Top bit of a non-intrinsic content id. Reserved intrinsic ids are `0..=THIS`
/// (all far below this bit), so content ids can never collide with them.
pub(super) const CONTENT_FLAG: u128 = 1u128 << 127;

/// 128-bit content hash: `FxHasher` (fixed seed) run twice with different
/// salts. Deterministic across processes and platforms.
pub(super) fn hash128<T: Hash + ?Sized>(value: &T) -> u128 {
    let mut lo = FxHasher::default();
    value.hash(&mut lo);
    let mut hi = FxHasher::default();
    0x9E37_79B9_7F4A_7C15u64.hash(&mut hi);
    value.hash(&mut hi);
    ((hi.finish() as u128) << 64) | (lo.finish() as u128)
}

pub(super) fn content_id(kind: &InternedTypeKind) -> CheckerTyId {
    CheckerTyId(hash128(kind) | CONTENT_FLAG)
}

pub(super) fn content_list_id(tys: &[CheckerTyId]) -> TyListId {
    TyListId(hash128(tys) | CONTENT_FLAG)
}

pub(super) fn content_function_id(f: &FunctionType) -> FunctionTypeId {
    FunctionTypeId(hash128(f) | CONTENT_FLAG)
}

pub(super) fn content_object_id(members: &[ObjectTypeMember]) -> ObjectMembersId {
    ObjectMembersId(hash128(members) | CONTENT_FLAG)
}
