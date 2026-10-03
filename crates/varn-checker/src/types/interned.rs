//! Hash-consed, **content-addressed** type table for the checker (Fase 1,
//! Componente 3 + ADR-0012).
//!
//! `varn_core::TypeKind<T, N, C, F, O, E>` is generic over how recursion,
//! names and collections are represented. This module fixes those
//! parameters to interned handles:
//!
//! - `T` (recursion, e.g. `Array(T)`, `KeyOf(T)`)      -> `CheckerTyId`
//! - `N` (name, e.g. `Named(N, Option<N>)`)              -> `varn_core::Atom`
//! - `C` (collection, e.g. `Union(C)`, `Tuple(C)`)       -> `TyListId`
//! - `F` (function shape)                                -> `FunctionTypeId`
//! - `O` (object members)                                -> `ObjectMembersId`
//! - `E` (today `()` in `SemanticTypeKind`)              -> `()`
//!
//! ## Content-addressed identity (the Ley 2/3 root-cause fix)
//!
//! A `CheckerTyId` is the **128-bit hash of the shape it names**, not a
//! positional index into a table. Two tables that intern the same shape in any
//! order — or in parallel — produce the **same id** for it, so ids are portable
//! by construction and no `absorb`/`reintern` remap is needed: merging tables is
//! a commutative, idempotent union. The ~13 intrinsic shapes keep a reserved
//! id range (`0..=THIS`) because `Type::Int`/`Type::Str`/... are `const`.
//! Non-intrinsic ids set the top bit, so they can never collide with that range.
//!
//! The hash is XXH3-128 over the shape's `Hash` stream; names inside a shape
//! are content-addressed `Atom`s, so the id never depends on interning order.
//!
//! `FunctionTypeId`/`ObjectMembersId` are content hashes of the function shape
//! and the member vector, so a shape's id is a Merkle hash over its children.

mod hash;
mod ids;
mod interning;
mod merge;
mod resolution;
mod slice;
mod table;

pub use ids::{CheckerTyId, FunctionTypeId, InternedTypeKind, ObjectMembersId, TyListId};
pub use slice::TySlice;
pub use table::CheckerTyTable;
