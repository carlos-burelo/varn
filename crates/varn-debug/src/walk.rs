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
            PoolEntry::Literal(Literal::Str(_))
            | PoolEntry::Literal(Literal::BigInt(_))
            | PoolEntry::Literal(Literal::Decimal(_))
            | PoolEntry::Literal(Literal::Char(_))
            | PoolEntry::Function(_)
            | PoolEntry::Shape(_)
            | PoolEntry::Layout(_)
            | PoolEntry::Variant(_) => VmValue::null(),
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
