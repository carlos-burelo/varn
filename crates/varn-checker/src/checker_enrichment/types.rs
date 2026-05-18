use crate::types::Type;

pub(super) fn join_types(mut types: Vec<Type>) -> Type {
    if types.is_empty() {
        return Type::Void;
    }

    let mut i = 0;
    while i < types.len() {
        let mut j = i + 1;
        while j < types.len() {
            if types[i] == types[j] {
                types.remove(j);
            } else {
                j += 1;
            }
        }
        i += 1;
    }

    if types.len() == 1 {
        types.pop().unwrap()
    } else {
        Type::union(types)
    }
}
