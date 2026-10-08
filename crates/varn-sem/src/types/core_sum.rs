use super::{CheckerTyTable, Type};
use varn_core::{Atom, CoreSum, TypeKind};

impl Type {
    pub fn core_sum<S: AsRef<str>>(
        &self,
        table: &CheckerTyTable,
        resolve: impl Fn(Atom) -> Option<S>,
    ) -> Option<(CoreSum, Vec<Type>)> {
        let TypeKind::Generic(name, args, origin) = table.get(self.0) else {
            return None;
        };
        let origin = origin.and_then(&resolve);
        let sum = CoreSum::identify(resolve(name)?.as_ref(), origin.as_ref().map(AsRef::as_ref))?;
        let args = table
            .get_list(args)
            .iter()
            .map(|&id| Type::resolved(id))
            .collect();
        Some((sum, args))
    }
}
