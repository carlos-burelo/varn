use crate::bind::BindResult;
use crate::exports::ExportMap;
use crate::portable::PortableModule;
use crate::types::CheckerTyTable;
use std::sync::Arc;
use varn_core::AtomInterner;

pub fn serialize_module_interface(
    exports: &ExportMap,
    bind: &BindResult,
) -> Result<Vec<u8>, String> {
    let mut names = bind.interner.clone();
    names.absorb(exports.table.names());
    let mut table = (*exports.table).clone();
    table.absorb(&bind.ty_table);
    let cached = PortableModule::from_live(exports, bind, &names, &table);
    postcard::to_allocvec(&cached).map_err(|e| e.to_string())
}

pub fn deserialize_module_interface(bytes: &[u8]) -> Result<(ExportMap, BindResult), String> {
    let cached: PortableModule = postcard::from_bytes(bytes).map_err(|e| e.to_string())?;
    let (mut exports, bind) =
        cached.into_live(&mut AtomInterner::new(), Arc::new(CheckerTyTable::new()));
    crate::exports::assign_slots(&mut exports);
    Ok((exports, bind))
}
