use super::tables::ClassInfo;
use rustc_hash::FxHashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlobalClassId(pub u32);

#[derive(Debug, Clone, Default)]
pub struct ClassRegistry {
    ids: FxHashMap<(Arc<str>, Arc<str>), GlobalClassId>,
    classes: Vec<ClassInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct ClassRegistryBuilder {
    entries: Vec<(Arc<str>, Arc<str>, bool, ClassInfo)>,
}

impl ClassRegistryBuilder {
    pub fn push_class(
        &mut self,
        defining_module: &str,
        name: &str,
        is_local_def: bool,
        info: ClassInfo,
    ) {
        self.entries.push((
            Arc::from(defining_module),
            Arc::from(name),
            is_local_def,
            info,
        ));
    }

    pub fn build(mut self) -> ClassRegistry {
        self.entries.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        let mut registry = ClassRegistry::default();
        for (module, name, is_local, info) in self.entries {
            if let Some(&id) = registry.ids.get(&(module.clone(), name.clone())) {
                if is_local {
                    registry.classes[id.0 as usize] = info;
                }
                continue;
            }
            let id = GlobalClassId(registry.classes.len() as u32);
            registry.ids.insert((module, name), id);
            registry.classes.push(info);
        }
        registry
    }
}

impl ClassRegistry {
    pub fn global_id(&self, defining_module: &str, name: &str) -> Option<GlobalClassId> {
        self.ids
            .get(&(Arc::from(defining_module), Arc::from(name)))
            .copied()
    }

    pub fn class(&self, id: GlobalClassId) -> Option<&ClassInfo> {
        self.classes.get(id.0 as usize)
    }

    pub fn len(&self) -> usize {
        self.classes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }
}
