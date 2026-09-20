//! Invariantes de `CheckerTyTable`.
//!
//! Un `CheckerTyId` es el **hash del contenido** de la forma que nombra, no un
//! índice posicional (ADR-0012). Por eso un id significa lo mismo en cualquier
//! tabla que haya internado la forma, sin importar el orden — y toda la
//! fragilidad de tipos entre módulos que se corrigió en `varn-checker` (ids que
//! se leían como otra forma, cachés que degradaban a `Dynamic`, bugs que
//! dependían del orden de bindeo) desaparece por construcción.
//!
//! Leyes que cubren (`AGENTS.md` §2): Ley 2 (los ids internos no cruzan
//! módulos — ya no hace falta que lo hagan: son portables), Ley 3 (una tabla,
//! un dueño) y Ley 4 (determinismo).

use std::sync::Arc;
use varn_checker::types::{CheckerTyId, CheckerTyTable, ObjectTypeMember, Type};
use varn_core::{TypeKind, TypeTag};

// ── Seeding intrínseco ────────────────────────────────────────────────────

/// Las ~21 formas intrínsecas tienen ids fijos y pequeños, para que
/// `Type::Int`/`Type::Str`/... puedan ser `const`. El resto son hashes.
#[test]
fn intrinsic_ids_are_fixed_and_small() {
    let t = CheckerTyTable::new();
    assert_eq!(t.get(CheckerTyId::INT), TypeKind::Intrinsic(TypeTag::Int));
    assert_eq!(t.get(CheckerTyId::STR), TypeKind::Intrinsic(TypeTag::Str));
    assert_eq!(t.get(CheckerTyId::BOOL), TypeKind::Intrinsic(TypeTag::Bool));
    assert_eq!(
        t.get(CheckerTyId::FLOAT),
        TypeKind::Intrinsic(TypeTag::Float)
    );
    assert_eq!(
        t.get(CheckerTyId::DYNAMIC),
        TypeKind::Intrinsic(TypeTag::Dynamic)
    );
    assert_eq!(t.get(CheckerTyId::THIS), TypeKind::This);
}

// ── Content addressing: el id no depende del orden ────────────────────────

/// La propiedad central: dos tablas que internan las mismas formas en
/// cualquier orden obtienen el MISMO id para cada forma. Antes esto era falso
/// (mismo índice, forma distinta) y era la causa raíz de la familia de bugs de
/// linaje de tipos.
#[test]
fn content_ids_are_order_independent() {
    let mut left = CheckerTyTable::new();
    let mut right = CheckerTyTable::new();

    let l_int = left.intern(TypeKind::Intrinsic(TypeTag::Int));
    let l_arr = left.intern(TypeKind::Array(l_int));
    let l_list = left.intern_list(&[l_int, CheckerTyId::STR]);
    let l_union = left.intern(TypeKind::Union(l_list));

    // `right` interna las mismas formas en orden distinto.
    let r_str = right.intern(TypeKind::Intrinsic(TypeTag::Str));
    let r_int = right.intern(TypeKind::Intrinsic(TypeTag::Int));
    let r_list = right.intern_list(&[r_int, r_str]);
    let r_union = right.intern(TypeKind::Union(r_list));
    let r_arr = right.intern(TypeKind::Array(r_int));

    assert_eq!(l_int, r_int);
    assert_eq!(l_arr, r_arr, "Array<int> es el mismo id en ambas tablas");
    assert_eq!(l_union, r_union, "Union<int,str> es el mismo id");
}

/// Internar dos veces la misma forma deduplica, y `get` devuelve la forma.
#[test]
fn intern_is_idempotent_and_get_roundtrips() {
    let mut t = CheckerTyTable::new();
    let a = t.intern(TypeKind::Array(CheckerTyId::INT));
    let b = t.intern(TypeKind::Array(CheckerTyId::STR));
    let a_again = t.intern(TypeKind::Array(CheckerTyId::INT));

    assert_eq!(a, a_again);
    assert_ne!(a, b);
    assert_eq!(t.get(a), TypeKind::Array(CheckerTyId::INT));
    assert_eq!(t.get(b), TypeKind::Array(CheckerTyId::STR));
}

// ── absorb: unión conmutativa de tablas ───────────────────────────────────

/// `absorb` es una unión: los ids foráneos ya son válidos (mismo contenido ⇒
/// mismo id), así que no hay remapeo. Antes era un `reintern` recursivo.
#[test]
fn absorb_is_a_union_that_preserves_ids() {
    let mut local = CheckerTyTable::new();
    let local_arr = local.intern(TypeKind::Array(CheckerTyId::INT));

    let mut foreign = CheckerTyTable::new();
    let foreign_list = foreign.intern_list(&[CheckerTyId::INT, CheckerTyId::STR]);
    let foreign_union = foreign.intern(TypeKind::Union(foreign_list));
    let foreign_members = foreign.intern_object_members(vec![ObjectTypeMember::Property {
        name: Arc::from("ok"),
        ty: CheckerTyId::BOOL,
        optional: false,
        readonly: false,
    }]);
    let foreign_obj = foreign.intern(TypeKind::Object(foreign_members));

    local.absorb(&foreign);

    // Las formas foráneas ahora resuelven en `local`, con el MISMO id.
    assert_eq!(local.get(foreign_union), foreign.get(foreign_union));
    assert_eq!(local.get(foreign_obj), foreign.get(foreign_obj));
    assert_eq!(local.get(local_arr), TypeKind::Array(CheckerTyId::INT));
}

// ── Type: el flag `tainted` no es identidad ───────────────────────────────

#[test]
fn tainted_flag_is_not_part_of_type_identity() {
    let mut t = CheckerTyTable::new();
    let id = t.intern(TypeKind::Array(CheckerTyId::INT));
    let plain = Type(id, false);
    let tainted = Type(id, true);
    assert_eq!(plain.id(), tainted.id());
    assert_eq!(plain.kind(&t), tainted.kind(&t));
}

// ── Send + Sync (Ley 3: la tabla puede compartirse entre hilos) ────────────

#[test]
fn checker_ty_table_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CheckerTyTable>();
    assert_send_sync::<Type>();
}

/// `BindResult` es lo que un chequeo paralelo movería entre hilos.
#[test]
fn bind_result_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<varn_checker::BindResult>();
}

/// El resolver es el orquestador que un pool de workers compartiría.
#[test]
fn disk_resolver_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<varn_checker::module_resolver::DiskResolver>();
}

/// Internar las mismas formas en hilos distintos y en órdenes distintos debe
/// dar los mismos ids: es la propiedad que hace segura la unión entre workers
/// (no hay un punto de serialización ni un remapeo que pueda desincronizarse).
#[test]
fn ids_agree_across_threads_and_interning_order() {
    fn build(union_first: bool) -> (CheckerTyId, CheckerTyId, CheckerTyId) {
        let mut t = CheckerTyTable::new();
        if union_first {
            let l = t.intern_list(&[CheckerTyId::INT, CheckerTyId::STR]);
            let _u = t.intern(TypeKind::Union(l));
            let a = t.intern(TypeKind::Array(CheckerTyId::INT));
            let l2 = t.intern_list(&[CheckerTyId::INT, CheckerTyId::STR]);
            (a, t.intern(TypeKind::Union(l2)), t.intern(TypeKind::Array(a)))
        } else {
            let a = t.intern(TypeKind::Array(CheckerTyId::INT));
            let l = t.intern_list(&[CheckerTyId::INT, CheckerTyId::STR]);
            let u = t.intern(TypeKind::Union(l));
            let a2 = t.intern(TypeKind::Array(CheckerTyId::INT));
            (a2, u, t.intern(TypeKind::Array(a)))
        }
    }

    let (left, right) = std::thread::scope(|s| {
        let h1 = s.spawn(|| build(true));
        let h2 = s.spawn(|| build(false));
        (h1.join().unwrap(), h2.join().unwrap())
    });
    assert_eq!(left, right, "mismos ids en hilos y órdenes distintos");
}
