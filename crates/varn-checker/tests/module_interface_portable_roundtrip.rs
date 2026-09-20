//! Round-trip de la INTERFAZ de un módulo por el caché en disco, con tipos.
//!
//! Regresión de la Ley 2 (`AGENTS.md` §2): lo que cruza la frontera de módulo
//! no puede llevar un `CheckerTyId`. Antes de `PortableType`, todo tipo no
//! intrínseco de una interfaz cacheada se degradaba a `Dynamic` (ver
//! `cache.rs`), así que un miembro de clase cacheado "no existía" o se tipaba
//! contra la nada según si la corrida calentó el caché. Aquí se exige que la
//! forma sobreviva: `int` sigue siendo `int` tras el disco.

use std::fs;
use std::path::PathBuf;
use varn_checker::module_resolver::{DiskResolver, ImportResolver};

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "varn-portable-iface-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn cached_interface_keeps_real_member_types() {
    let dir = scratch_dir("member-types");
    let lib_path = dir.join("shapes.vn");
    fs::write(
        &lib_path,
        "export class Box {\n  value: int;\n  label: str;\n}\n\
         export function makeBox(): Box { return new Box(); }\n",
    )
    .expect("write shapes.vn");
    let lib_str = lib_path.to_string_lossy().into_owned();

    // Primer "proceso": bindea desde fuente y escribe la interfaz al caché.
    let resolver1 = DiskResolver::new();
    let bind1 = resolver1
        .module_bind(&lib_str)
        .expect("shapes.vn debe bindear");
    assert!(
        !bind1.diagnostics.has_errors(),
        "shapes.vn debe bindear limpio: {:?}",
        bind1.diagnostics.errors().collect::<Vec<_>>()
    );

    // Segundo "proceso": resolver nuevo, misma fuente -> sirve del caché.
    let resolver2 = DiskResolver::new();
    let bind2 = resolver2
        .module_bind(&lib_str)
        .expect("debe cargar del caché en un resolver nuevo");

    let box_members = bind2
        .type_members
        .classes
        .get("Box")
        .expect("`Box` debe sobrevivir el caché");
    let value = box_members
        .members
        .iter()
        .find(|m| m.name.as_ref() == "value")
        .expect("`Box.value` debe sobrevivir el caché");

    // La forma, no `Dynamic`: el id decodificado es válido en la tabla del
    // bind cargado.
    assert_eq!(
        bind2.ty_table.get(value.ty.0),
        varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Int),
        "el tipo de `Box.value` debe ser `int`, no degradarse a Dynamic"
    );

    let label = box_members
        .members
        .iter()
        .find(|m| m.name.as_ref() == "label")
        .expect("`Box.label` debe sobrevivir el caché");
    assert_eq!(
        bind2.ty_table.get(label.ty.0),
        varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Str),
        "el tipo de `Box.label` debe ser `str`"
    );

    // Y la firma exportada también conserva su retorno (`Box`, no Dynamic).
    let make = bind2
        .global_symbols()
        .find(|s| bind2.interner.resolve(s.name) == "makeBox")
        .and_then(|s| s.ty)
        .expect("`makeBox` debe tener tipo");
    let varn_core::TypeKind::Fn(fid) = bind2.ty_table.get(make.0) else {
        panic!("`makeBox` debe decodificar a Fn, no a Dynamic");
    };
    let ret = bind2.ty_table.get_function(fid).return_type;
    assert!(
        matches!(bind2.ty_table.get(ret), varn_core::TypeKind::Named(_, _)),
        "el retorno de `makeBox` debe ser `Box` (Named), no Dynamic"
    );

    let _ = fs::remove_dir_all(&dir);
}
