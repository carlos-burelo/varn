use rustc_hash::FxHashMap as HashMap;

#[derive(Debug, Clone, Default)]
pub struct ModuleClasses {
    pub classes: Vec<varn_tir::ClassInfo>,
    pub origins: Vec<Option<String>>,
    pub types: varn_tir::TyTable,
}

fn is_unboxed_kind(kind: varn_core::RuntimeKind) -> bool {
    use varn_core::RuntimeKind as K;
    matches!(kind, K::Int | K::Float | K::Bool | K::Char)
}

/// Los slots deben coincidir exacto. Los tipos solo importan por
/// representación: unboxed (Int/Float/Bool/Char) exige igualdad porque los
/// primitivos bajan idéntico en todo contexto; en boxed basta que la copia
/// sea igual o genérica (degrada a ByName, sano).
fn fields_cover(
    copy: &[varn_tir::FieldInfo],
    copy_types: &varn_tir::TyTable,
    def: &[varn_tir::FieldInfo],
    def_types: &varn_tir::TyTable,
) -> bool {
    if copy.len() != def.len() {
        return false;
    }
    copy.iter().zip(def.iter()).all(|(c, d)| {
        if c.name != d.name || c.slot != d.slot {
            return false;
        }
        match (c.ty.field_kind(copy_types), d.ty.field_kind(def_types)) {
            (Some(a), Some(b)) => a == b,
            (None, Some(b)) => !is_unboxed_kind(b),
            (None, None) => true,
            (Some(_), None) => false,
        }
    })
}

pub fn verify_foreign_layouts(tables: &HashMap<String, ModuleClasses>) -> Result<(), String> {
    let mut modules: Vec<&String> = tables.keys().collect();
    modules.sort();
    for module in modules {
        let entry = &tables[module];
        let mut foreign: Vec<(&str, &str)> = entry
            .origins
            .iter()
            .zip(entry.classes.iter())
            .filter_map(|(origin, ci)| origin.as_ref().map(|o| (ci.name.as_ref(), o.as_str())))
            .collect();
        foreign.sort();
        for (name, origin) in foreign {
            let Some(defining) = tables.get(origin) else {
                continue;
            };
            let Some(local) = defining
                .origins
                .iter()
                .zip(defining.classes.iter())
                .find(|(o, ci)| o.is_none() && ci.name.as_ref() == name)
                .map(|(_, ci)| ci)
            else {
                continue;
            };
            let copy = entry
                .origins
                .iter()
                .zip(entry.classes.iter())
                .find(|(o, ci)| o.as_deref() == Some(origin) && ci.name.as_ref() == name)
                .map(|(_, ci)| ci)
                .expect("foreign entry vanished");
            if !fields_cover(&copy.fields, &entry.types, &local.fields, &defining.types) {
                let show = |fields: &[varn_tir::FieldInfo]| {
                    fields
                        .iter()
                        .map(|f| format!("{}:{:?}#{}", f.name, f.ty, f.slot))
                        .collect::<Vec<_>>()
                        .join(",")
                };
                return Err(format!(
                    "E_CLASS_LAYOUT_DIVERGENCE: '{name}' copied into '{module}' from '{origin}' differs from its definition (copy [{}] vs def [{}])",
                    show(&copy.fields),
                    show(&local.fields)
                ));
            }
        }
    }
    Ok(())
}
