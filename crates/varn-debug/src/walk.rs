use varn_types::{FunctionProto, Literal, PoolEntry, VmValue};

pub fn constants_for_inspect(proto: &FunctionProto) -> Vec<VmValue> {
    proto
        .chunk
        .constants
        .iter()
        .map(|entry| match entry {
            PoolEntry::Literal(Literal::Null) => VmValue::null(),
            PoolEntry::Literal(Literal::Bool(b)) => VmValue::from_bool(*b),
            PoolEntry::Literal(Literal::Int(n)) => VmValue::from_int(*n),
            PoolEntry::Literal(Literal::Float(f)) => VmValue::from_f64(*f),

            _ => VmValue::null(),
        })
        .collect()
}

pub fn for_each_fn(root: &FunctionProto, f: &mut impl FnMut(usize, &FunctionProto)) {
    visit(root, 0, f);
}

fn visit(proto: &FunctionProto, depth: usize, f: &mut impl FnMut(usize, &FunctionProto)) {
    f(depth, proto);
    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(inner) = entry {
            visit(inner, depth + 1, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proto_with(constants: Vec<PoolEntry>) -> FunctionProto {
        let mut p = FunctionProto::default();
        p.chunk.constants = constants;
        p
    }

    #[test]
    fn constants_map_scalars_and_null_heap() {
        let p = proto_with(vec![
            PoolEntry::Literal(Literal::Null),
            PoolEntry::Literal(Literal::Bool(true)),
            PoolEntry::Literal(Literal::Int(7)),
            PoolEntry::Literal(Literal::Float(1.5)),
        ]);
        let c = constants_for_inspect(&p);
        assert_eq!(c.len(), 4);
        assert!(c[0].is_null());
        assert_eq!(c[2].as_int(), 7);
    }

    #[test]
    fn for_each_fn_visits_nested_depths() {
        let inner = proto_with(vec![]);
        let root = proto_with(vec![PoolEntry::Function(std::rc::Rc::new(inner))]);
        let mut seen = Vec::new();
        for_each_fn(&root, &mut |depth, _| seen.push(depth));
        assert_eq!(seen, vec![0, 1]);
    }
}
