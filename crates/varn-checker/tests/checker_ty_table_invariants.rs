//! Invariantes de `CheckerTyTable`.
//!
//! Un `CheckerTyId` solo significa algo dentro de la tabla que lo internó. Toda
//! la fragilidad de tipos entre módulos que se corrigió en `varn-checker`
//! (ids que se leen como otra forma, cachés que degradan a `Dynamic`, bugs que
//! dependen del orden de bindeo) son violaciones de alguna de estas
//! propiedades. Estos tests las fijan para que no vuelvan.
//!
//! Leyes que cubren (`AGENTS.md` §2): Ley 2 (los ids internos no cruzan
//! módulos) y Ley 3 (una tabla, un dueño).

use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_checker::types::{
    CheckerTyId, CheckerTyTable, ObjectTypeMember, Type,
};
use varn_core::{AtomInterner, TypeKind, TypeTag};

// ── Seeding intrínseco ────────────────────────────────────────────────────

/// Las ~21 formas intrínsecas se siembran en orden fijo, así que sus ids son
/// válidos en *cualquier* tabla. Si esto se rompe, todo lo demás miente.
#[test]
fn intrinsic_ids_are_fixed_and_portable() {
    let t = CheckerTyTable::new();
    assert_eq!(t.get(CheckerTyId::INT), &TypeKind::Intrinsic(TypeTag::Int));
    assert_eq!(t.get(CheckerTyId::STR), &TypeKind::Intrinsic(TypeTag::Str));
    assert_eq!(t.get(CheckerTyId::BOOL), &TypeKind::Intrinsic(TypeTag::Bool));
    assert_eq!(t.get(CheckerTyId::FLOAT), &TypeKind::Intrinsic(TypeTag::Float));
    assert_eq!(
        t.get(CheckerTyId::DYNAMIC),
        &TypeKind::Intrinsic(TypeTag::Dynamic)
    );
    assert_eq!(t.get(CheckerTyId::THIS), &TypeKind::This);

    assert!(CheckerTyId::INT.is_portable());
    assert!(CheckerTyId::THIS.is_portable());
}

/// Un id que no es intrínseco NO es portable: solo vale en su tabla.
#[test]
fn non_intrinsic_ids_are_not_portable() {
    let mut t = CheckerTyTable::new();
    let seeded = t.len();
    let id = t.intern(TypeKind::Array(CheckerTyId::INT));
    assert_eq!(id.index() as usize, seeded, "el primer id de usuario va justo después del seeding");
    assert!(!id.is_portable());
}

/// `sanitize_foreign` degrada a `Dynamic` solo lo no portable.
#[test]
fn sanitize_foreign_keeps_intrinsics_and_degrades_the_rest() {
    assert_eq!(CheckerTyId::STR.sanitize_foreign(), CheckerTyId::STR);
    assert_eq!(CheckerTyId::DYNAMIC.sanitize_foreign(), CheckerTyId::DYNAMIC);

    let mut t = CheckerTyTable::new();
    let foreign = t.intern(TypeKind::Array(CheckerTyId::INT));
    assert_eq!(foreign.sanitize_foreign(), CheckerTyId::DYNAMIC);
}

// ── intern: append-only y hash-consing ────────────────────────────────────

/// Internar dos veces la misma forma da el mismo id, y un id ya entregado
/// nunca cambia de forma (la tabla solo crece).
#[test]
fn intern_is_append_only_and_dedups() {
    let mut t = CheckerTyTable::new();
    let a = t.intern(TypeKind::Array(CheckerTyId::INT));
    let b = t.intern(TypeKind::Array(CheckerTyId::STR));
    let a_again = t.intern(TypeKind::Array(CheckerTyId::INT));

    assert_eq!(a, a_again, "la misma forma deduplica al mismo id");
    assert_ne!(a, b);
    assert_eq!(t.get(a), &TypeKind::Array(CheckerTyId::INT));
    assert_eq!(t.get(b), &TypeKind::Array(CheckerTyId::STR));
}

// ── divergencia: por qué un id crudo no puede cruzar ──────────────────────

/// Dos tablas que crecieron desde la misma semilla pueden tener formas
/// DISTINTAS en el MISMO índice. `len()` idéntico no implica compatibilidad:
/// este es exactamente el escenario que hacía que un tipo de un módulo se
/// leyera como otro en el módulo vecino.
#[test]
fn divergent_tables_share_an_index_but_not_its_meaning() {
    let atoms = {
        let mut i = AtomInterner::new();
        i.intern("Sender")
    };

    let mut a = CheckerTyTable::new();
    let named = a.intern(TypeKind::Named(atoms, None));

    let mut b = CheckerTyTable::new();
    let list = b.intern_list(&[CheckerTyId::INT, CheckerTyId::STR]);
    let union = b.intern(TypeKind::Union(list));

    assert_eq!(a.len(), b.len(), "misma longitud...");
    assert_eq!(named.index(), union.index(), "...y mismo índice...");

    assert!(matches!(a.get(named), TypeKind::Named(_, _)), "...pero A dice Named");
    assert!(matches!(b.get(union), TypeKind::Union(_)), "...y B dice Union");
}

// ── reintern: traducir una forma foránea a la tabla propia ────────────────

/// Traducir una forma foránea no la lee cruda: la re-interna por estructura,
/// recursivamente. Un id portable se traduce a sí mismo.
#[test]
fn reintern_translates_recursively_and_keeps_portable_ids() {
    let mut foreign = CheckerTyTable::new();
    let inner = foreign.intern_list(&[CheckerTyId::INT, CheckerTyId::STR]);
    let union = foreign.intern(TypeKind::Union(inner));
    let array = foreign.intern(TypeKind::Array(union));

    let mut local = CheckerTyTable::new();
    let mut cache = FxHashMap::default();

    assert_eq!(
        local.reintern(&foreign, CheckerTyId::INT, &mut cache),
        CheckerTyId::INT,
        "un id portable es el mismo en cualquier tabla"
    );

    let translated = local.reintern(&foreign, array, &mut cache);
    let TypeKind::Array(element) = local.get(translated) else {
        panic!("debe traducirse a un Array local, no copiarse el índice");
    };
    let TypeKind::Union(members) = local.get(*element) else {
        panic!("el elemento anidado también se traduce");
    };
    assert_eq!(
        local.get_list(*members),
        &[CheckerTyId::INT, CheckerTyId::STR],
        "la recursión preserva los miembros"
    );
}

/// Traducir no muta la tabla origen.
#[test]
fn reintern_does_not_touch_the_source_table() {
    let mut foreign = CheckerTyTable::new();
    let array = foreign.intern(TypeKind::Array(CheckerTyId::INT));
    let before = foreign.len();

    let mut local = CheckerTyTable::new();
    let local_before = local.len();
    let mut cache = FxHashMap::default();
    let translated = local.reintern(&foreign, array, &mut cache);

    assert_eq!(foreign.len(), before, "la tabla foránea queda intacta");
    assert!(local.len() > local_before, "la forma se internó en la local");
    assert!(matches!(local.get(translated), TypeKind::Array(_)));
}

/// `reintern` traduce también los miembros de un objeto (Property/Method/Index).
#[test]
fn reintern_translates_object_members() {
    let mut foreign = CheckerTyTable::new();
    let members = foreign.intern_object_members(vec![
        ObjectTypeMember::Property {
            name: Rc::from("ok"),
            ty: CheckerTyId::BOOL,
            optional: false,
            readonly: false,
        },
        ObjectTypeMember::Index {
            param_name: Rc::from("k"),
            key_ty: CheckerTyId::STR,
            value_ty: CheckerTyId::INT,
        },
    ]);
    let object = foreign.intern(TypeKind::Object(members));

    let mut local = CheckerTyTable::new();
    let mut cache = FxHashMap::default();
    let translated = local.reintern(&foreign, object, &mut cache);

    let TypeKind::Object(mid) = local.get(translated) else {
        panic!("debe ser un Object local");
    };
    let local_members = local.get_object_members(*mid);
    let ObjectTypeMember::Property { name, ty, .. } = &local_members[0] else {
        panic!("primer miembro es Property");
    };
    assert_eq!(name.as_ref(), "ok");
    assert_eq!(*ty, CheckerTyId::BOOL);
    let ObjectTypeMember::Index { key_ty, value_ty, .. } = &local_members[1] else {
        panic!("segundo miembro es Index");
    };
    assert_eq!(*key_ty, CheckerTyId::STR);
    assert_eq!(*value_ty, CheckerTyId::INT);
}

// ── absorb: aprender formas sin repuntar los ids propios ──────────────────

/// `absorb` conserva el significado de los ids que `self` ya entregó, aunque
/// `other` tenga otra forma en ese índice, y aprende las que le faltan.
#[test]
fn absorb_keeps_local_indices_and_learns_missing_shapes() {
    let mut a = CheckerTyTable::new();
    let shared = a.intern(TypeKind::Array(CheckerTyId::INT));

    let mut b = a.clone();
    let a_list = a.intern_list(&[CheckerTyId::INT]);
    let a_only = a.intern(TypeKind::Tuple(a_list));
    let b_list = b.intern_list(&[CheckerTyId::STR]);
    let b_only = b.intern(TypeKind::Union(b_list));

    assert_eq!(
        a_only.index(),
        b_only.index(),
        "crecieron en paralelo: mismo índice, formas distintas"
    );

    b.absorb(&a);

    // El prefijo compartido sigue igual.
    assert_eq!(b.get(shared), &TypeKind::Array(CheckerTyId::INT));
    // El índice divergente conserva el significado de `b`, no el de `a`.
    assert!(
        matches!(b.get(b_only), TypeKind::Union(_)),
        "absorb no debe repuntar un id que `b` ya entregó"
    );
    // Y la forma que solo tenía `a` ahora existe en `b`.
    let a_list_again = b.intern_list(&[CheckerTyId::INT]);
    let learned = b.intern(TypeKind::Tuple(a_list_again));
    assert!(matches!(b.get(learned), TypeKind::Tuple(_)));
    assert!(learned.index() > b_only.index());
}

// ── Type: el flag `tainted` no es identidad ───────────────────────────────

/// El bool de `Type` (tainted) no participa del hash-consing: dos `Type` con
/// el mismo id y distinto flag comparten forma.
/// El modelo de snapshots (Binder/Checker clonan la tabla viva y crecen) es
/// seguro **solo mientras cada copia mantenga la tabla viva como prefijo**.
/// Este test fija esa invariante, que es la base para el dueño único pendiente
/// (Ley 3): hoy se conserva por construcción (`absorb` solo agrega), y así se
/// comprueba en vez de asumirse.
#[test]
fn cloned_tables_keep_the_live_table_as_prefix_until_they_diverge() {
    let mut live = CheckerTyTable::new();
    let a = live.intern(TypeKind::Array(CheckerTyId::INT));

    let snapshot = live.clone();
    assert!(live.has_prefix(&snapshot), "clon comparte el prefijo");

    // `live` crece: sigue teniendo al snapshot como prefijo.
    let b = live.intern(TypeKind::Array(CheckerTyId::STR));
    assert!(live.has_prefix(&snapshot));
    assert!(!snapshot.has_prefix(&live), "el snapshot NO ve el crecimiento");

    // `absorb` restaura la relación: el snapshot aprende lo que le faltaba sin
    // repuntar sus propios ids.
    let mut snapshot2 = snapshot.clone();
    snapshot2.absorb(&live);
    assert!(snapshot2.has_prefix(&live));
    assert_eq!(snapshot2.get(a), &TypeKind::Array(CheckerTyId::INT));
    assert_eq!(snapshot2.get(b), &TypeKind::Array(CheckerTyId::STR));

    // Dos tablas que crecieron en paralelo divergen en el mismo índice.
    let mut left = CheckerTyTable::new();
    let mut right = CheckerTyTable::new();
    let ll = left.intern_list(&[CheckerTyId::INT]);
    let l = left.intern(TypeKind::Tuple(ll));
    let rl = right.intern_list(&[CheckerTyId::STR]);
    let r = right.intern(TypeKind::Union(rl));
    assert_eq!(l.index(), r.index());
    assert!(!left.has_prefix(&right), "mismo índice, forma distinta");
}

#[test]
fn tainted_flag_is_not_part_of_type_identity() {
    let mut t = CheckerTyTable::new();
    let id = t.intern(TypeKind::Array(CheckerTyId::INT));
    let plain = Type(id, false);
    let tainted = Type(id, true);
    assert_eq!(plain.id(), tainted.id());
    assert_eq!(plain.kind(&t), tainted.kind(&t));
}
