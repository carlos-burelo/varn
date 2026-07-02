use varn_types::value::{
    install_allocator, AllocVtable, ArrayRef, MapRef, ObjData, ObjRef, SetRef, Value, ValueMap,
    ValueSet,
};

fn make_object() -> ObjRef {
    ObjRef::new(ObjData::new())
}

fn make_array() -> ArrayRef {
    ArrayRef::new(Vec::<Value>::new())
}

fn make_map() -> MapRef {
    MapRef::new(ValueMap::default())
}

fn make_set() -> SetRef {
    SetRef::new(ValueSet::default())
}

static VTABLE: AllocVtable = AllocVtable {
    alloc_object: make_object,
    alloc_array: make_array,
    alloc_map: make_map,
    alloc_set: make_set,
};

pub fn init_heap() {
    varn_types::value::register_global_vtable(&VTABLE);
    install_allocator(&VTABLE);
}
