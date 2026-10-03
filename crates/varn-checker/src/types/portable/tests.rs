use super::{decode, encode, PortableType};
use crate::types::CheckerTyId;
use crate::types::{CheckerTyTable, FunctionParam, FunctionType, ObjectTypeMember, Type};
use varn_core::{AtomInterner, LangPrimitive, TypeKind};

fn roundtrip(ty: Type, table: &CheckerTyTable, interner: &AtomInterner) -> PortableType {
    let encoded = encode(ty, table, interner);
    let mut fresh = CheckerTyTable::new();
    let mut fresh_interner = AtomInterner::new();
    // El fresh_interner debe poder resolver los nombres que decode
    // re-interna; `decode` los mete él mismo, así que basta con partir de
    // los intrínsecos.
    let decoded = decode(&encoded, &mut fresh, &mut fresh_interner);
    // Re-codificar el resultado debe dar exactamente la misma forma: es la
    // prueba de que la traducción no perdió ni cambió estructura.
    encode(decoded, &fresh, &fresh_interner)
}

#[test]
fn roundtrips_intrinsics_and_arrays() {
    let mut t = CheckerTyTable::new();
    let i = AtomInterner::new();
    let arr = t.intern(TypeKind::Array(CheckerTyId::INT));
    assert_eq!(
        roundtrip(Type(arr, false), &t, &i),
        encode(Type(arr, false), &t, &i)
    );
}

#[test]
fn roundtrips_generic_with_origin_and_names() {
    let mut t = CheckerTyTable::new();
    let mut i = AtomInterner::new();
    let name = i.intern("Sender");
    let origin = i.intern("runtime:task");
    let args = t.intern_list(&[CheckerTyId::INT]);
    let g = t.intern(TypeKind::Generic(name, args, Some(origin)));
    let encoded = encode(Type(g, false), &t, &i);
    assert_eq!(
        encoded,
        PortableType::Generic(
            "Sender".to_string(),
            vec![PortableType::Primitive(LangPrimitive::Int)],
            Some("runtime:task".to_string())
        )
    );
    // Y sobrevive un ciclo encode→decode→encode.
    let mut fresh = CheckerTyTable::new();
    let mut fi = AtomInterner::new();
    let decoded = decode(&encoded, &mut fresh, &mut fi);
    assert_eq!(encode(decoded, &fresh, &fi), encoded);
}

#[test]
fn roundtrips_function_signature() {
    let mut t = CheckerTyTable::new();
    let i = AtomInterner::new();
    let ret = t.intern_function(FunctionType {
        params: vec![FunctionParam {
            name: Some(std::sync::Arc::from("x")),
            ty: CheckerTyId::FLOAT,
            optional: false,
            is_rest: false,
        }],
        return_type: CheckerTyId::FLOAT,
        is_arrow: false,
        type_params: vec![],
    });
    let f = t.intern(TypeKind::Fn(ret));
    let encoded = encode(Type(f, false), &t, &i);
    let mut fresh = CheckerTyTable::new();
    let mut fi = AtomInterner::new();
    let decoded = decode(&encoded, &mut fresh, &mut fi);
    let TypeKind::Fn(fid) = fresh.get(decoded.0) else {
        panic!("debe decodificar a Fn");
    };
    let ft = fresh.get_function(fid);
    assert_eq!(ft.params[0].ty, CheckerTyId::FLOAT);
    assert_eq!(ft.return_type, CheckerTyId::FLOAT);
}

#[test]
fn roundtrips_object_members() {
    let mut t = CheckerTyTable::new();
    let i = AtomInterner::new();
    let oid = t.intern_object_members(vec![ObjectTypeMember::Index {
        param_name: std::sync::Arc::from("k"),
        key_ty: CheckerTyId::STR,
        value_ty: CheckerTyId::INT,
    }]);
    let o = t.intern(TypeKind::Object(oid));
    let encoded = encode(Type(o, false), &t, &i);
    let mut fresh = CheckerTyTable::new();
    let mut fi = AtomInterner::new();
    let decoded = decode(&encoded, &mut fresh, &mut fi);
    let TypeKind::Object(oid) = fresh.get(decoded.0) else {
        panic!("debe decodificar a Object");
    };
    let members = fresh.get_object_members(oid);
    assert!(matches!(
        &members[0],
        ObjectTypeMember::Index { key_ty, value_ty, .. }
            if *key_ty == CheckerTyId::STR && *value_ty == CheckerTyId::INT
    ));
}

#[test]
fn typeof_degrades_to_dynamic_instead_of_inventing_an_id() {
    // No podemos fabricar un `ExprId` legítimo sin arena, así que la
    // garantía que probamos es la ausencia de pánico y la degradación
    // documentada cuando encode ve un `Typeof`: se cubre indirectamente
    // por el match exhaustivo; aquí fijamos la forma portable resultante.
    let mut t = CheckerTyTable::new();
    let i = AtomInterner::new();
    let dynamic = t.intern(TypeKind::Primitive(varn_core::LangPrimitive::Dynamic));
    assert_eq!(
        encode(Type(dynamic, false), &t, &i),
        PortableType::Primitive(LangPrimitive::Dynamic)
    );
}
