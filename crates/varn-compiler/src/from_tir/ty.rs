//! `varn_tir::BackendTy` -> the SSA IR's type (`crate::hir::HirType`).
//!
//! The SSA IR keeps its current type enum through stage 3 (it is renamed off
//! `Hir` once HIR is deleted). `BackendTy`'s structured handles index the TIR
//! module's own `TyTable`; here they are re-interned into the SSA-side
//! `TyTable`, which is keyed by class *name*.
//!
//! Kinds the SSA type has no slot for (`char`, `decimal`, `bigint`, `enum`,
//! `fn`, `map`, `set`, `tuple`) map to `Ref` — a heap value with no further
//! static shape. That is conservative, never wrong; the precise opcodes those
//! types unlock are stage 4/5.

use crate::hir::{HirType, TyTable as SsaTyTable};
use varn_tir::{BackendTy, TirModule};

/// Lower one `BackendTy`, re-interning any nested handles into `out`.
pub fn lower(bt: BackendTy, tir: &TirModule, out: &mut SsaTyTable) -> HirType {
    match bt {
        BackendTy::Int => HirType::Int,
        // Los anchos angostos viven en el MISMO registro físico que `Int`/
        // `Float` (GPR de 64 bits / FPR): el ancho no es una clase de
        // registro distinta, solo cambia dónde se valida el rango
        // (`InstKind::NarrowRangeCheck`, insertado en `from_tir/build.rs`
        // usando el `BackendTy` original, ANTES de perderlo aquí) y cómo se
        // empaca en un campo/array (`class_field_repr`, que lee el
        // `TypeTag` real vía `field_tag`, no este `HirType`).
        BackendTy::Int8
        | BackendTy::Int16
        | BackendTy::Int32
        | BackendTy::UInt8
        | BackendTy::UInt16
        | BackendTy::UInt32 => HirType::Int,
        BackendTy::Float => HirType::Float,
        BackendTy::Float32 => HirType::Float,
        BackendTy::Bool => HirType::Bool,
        BackendTy::Str => HirType::Str,
        // `char` es `HeapObj::Char` internado: `Ref` es la
        // proyección honesta (clase REF, con flush GC). `Dynamic` era
        // conservador pero perdía la clase; `Int` (que usaba `build.rs` para
        // el literal) era directamente falso.
        BackendTy::Char => HirType::Ref,
        // `decimal`/`bigint` son heap PERO con ensanchado `int` en llamadas
        // (`coherence::assignable`: `Int` → `Decimal`/`BigInt`) y tolerancia
        // `dynamic`: un registro REF no puede alojar el `int` sin allocar.
        // `Dynamic` los aloja tal cual (igual que hoy el stack universal) y
        // los opcodes genéricos ya los resuelven por tag.
        BackendTy::Decimal | BackendTy::BigInt => HirType::Dynamic,

        BackendTy::Bytes | BackendTy::Tuple(_) | BackendTy::Enum(_) | BackendTy::Fn(_) => {
            HirType::Ref
        }

        BackendTy::Map(k, v) => {
            let k_inner = resolve(k, tir, out);
            let v_inner = resolve(v, tir, out);
            HirType::Map(out.intern(k_inner), out.intern(v_inner))
        }
        BackendTy::Set(el) => {
            let inner = resolve(el, tir, out);
            HirType::Set(out.intern(inner))
        }

        BackendTy::Array(el) => {
            let inner = resolve(el, tir, out);
            HirType::Array(out.intern(inner))
        }
        BackendTy::Class(cid) => match tir.class(cid) {
            Some(ci) => HirType::Class(out.class_id(&ci.name)),
            None => HirType::Ref,
        },
        BackendTy::Nullable(inner) => {
            let inner = resolve(inner, tir, out);
            HirType::Nullable(out.intern(inner))
        }

        BackendTy::Void | BackendTy::Never | BackendTy::Dynamic(_) => HirType::Dynamic,
    }
}

fn resolve(id: varn_tir::TyId, tir: &TirModule, out: &mut SsaTyTable) -> HirType {
    if !tir.types.contains(id) {
        return HirType::Dynamic;
    }
    lower(tir.types.get(id), tir, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use varn_tir::{
        BackendTy as B, ClassInfo, DynReason, Signature, TirFunction, TirModule, TyTable,
    };

    fn empty_module() -> TirModule {
        let mut types = TyTable::default();
        let _ = types.intern(B::Never);
        TirModule {
            source_file: Rc::from("t.vn"),
            imports: vec![],
            exports: vec![],
            types,
            classes: vec![ClassInfo::new(Rc::from("Point"), None, vec![])],
            enums: vec![],
            signatures: vec![Signature {
                params: vec![],
                return_ty: B::Void,
            }],
            functions: vec![],
            globals: vec![],
            global_names: vec![],
            class_defs: vec![],
            top_level: TirFunction {
                name: Rc::from("<module>"),
                sig: varn_tir::SigId(0),
                params: vec![],
                return_ty: B::Void,
                locals: vec![],
                body: vec![],
                has_this: false,
                this_class: None,
                is_async: false,
                is_generator: false,
                has_rest: false,
            },
        }
    }

    #[test]
    fn scalars_pass_through() {
        let m = empty_module();
        let mut out = SsaTyTable::default();
        assert_eq!(lower(B::Int, &m, &mut out), HirType::Int);
        assert_eq!(lower(B::Bool, &m, &mut out), HirType::Bool);
        assert_eq!(lower(B::Str, &m, &mut out), HirType::Str);
    }

    #[test]
    fn dynamic_and_opaque_kinds_are_dynamic_or_ref() {
        let m = empty_module();
        let mut out = SsaTyTable::default();
        assert_eq!(
            lower(B::Dynamic(DynReason::Unannotated), &m, &mut out),
            HirType::Dynamic
        );
        assert_eq!(lower(B::Decimal, &m, &mut out), HirType::Dynamic);
        assert_eq!(lower(B::Void, &m, &mut out), HirType::Dynamic);
    }

    #[test]
    fn heap_boxed_scalars_are_ref() {
        // `char` es `HeapObj::Char` internado: `Ref`, nunca `Int` (mentía al
        // regalloc/GC) ni `Dynamic` (degradaba la clase sin motivo).
        // `decimal`/`bigint`, en cambio, aceptan `int` por ensanchado:
        // `Dynamic` los aloja sin allocar.
        let m = empty_module();
        let mut out = SsaTyTable::default();
        assert_eq!(lower(B::Char, &m, &mut out), HirType::Ref);
        assert_eq!(lower(B::Decimal, &m, &mut out), HirType::Dynamic);
        assert_eq!(lower(B::BigInt, &m, &mut out), HirType::Dynamic);
        assert_eq!(lower(B::Bytes, &m, &mut out), HirType::Ref);
    }

    #[test]
    fn array_of_int_re_interns() {
        let mut m = empty_module();
        let int_id = m.types.intern(B::Int);
        let mut out = SsaTyTable::default();
        let HirType::Array(el) = lower(B::Array(int_id), &m, &mut out) else {
            panic!("expected Array");
        };
        assert_eq!(out.get(el), HirType::Int);
    }

    #[test]
    fn class_maps_by_name() {
        let m = empty_module();
        let mut out = SsaTyTable::default();
        let HirType::Class(cid) = lower(B::Class(varn_tir::ClassId(0)), &m, &mut out) else {
            panic!("expected Class");
        };
        assert_eq!(out.class_name(cid).as_ref(), "Point");
    }
}
