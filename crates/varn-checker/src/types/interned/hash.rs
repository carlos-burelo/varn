use super::ids::{CheckerTyId, FunctionTypeId, InternedTypeKind, ObjectMembersId, TyListId};
use crate::types::{FunctionType, ObjectTypeMember};
use std::hash::Hash;
use xxhash_rust::xxh3::Xxh3;

/// Top bit of a non-intrinsic content id. Reserved intrinsic ids are `0..=THIS`
/// (all far below this bit), so content ids can never collide with them.
pub(super) const CONTENT_FLAG: u128 = 1u128 << 127;

pub(super) fn hash128<T: Hash + ?Sized>(value: &T) -> u128 {
    let mut h = Xxh3::new();
    value.hash(&mut h);
    h.digest128()
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
