#![allow(unused_crate_dependencies)]

use std::collections::BTreeMap;
use varn_core::OpCode;
use varn_types::ssa::SsaOp;
use varn_types::{FunctionProto, Literal, PoolEntry};

const SOURCE: &str = r#"
function walk(o: dynamic, n: int): dynamic {
    let acc: dynamic = o.start
    let i = 0
    while (i < n) {
        if (o.flag) {
            o.hits = i
        } else {
            o.misses = i
        }
        i = i + 1
    }
    o.done = true
    return o.result
}
print(walk({ start: 0, flag: true, result: 1 }, 3))
"#;

type Sites = BTreeMap<(String, u16), usize>;

fn bytecode_sites(proto: &FunctionProto) -> Sites {
    let code = &proto.chunk.code;
    let consts = &proto.chunk.constants;
    let mut sites = Sites::new();
    let mut ip = 0;
    while ip < code.len() {
        let info = varn_types::bytecode::decode(code, ip, consts).expect("decodable bytecode");
        let op = OpCode::from_u16(code[ip]).expect("opcode");
        if matches!(op, OpCode::GetProperty | OpCode::SetProperty) {
            let slot = code[ip + 1] & 0xFF;
            let name = match &consts[code[ip + 2] as usize] {
                PoolEntry::Literal(Literal::Str(s)) => s.to_string(),
                other @ (PoolEntry::Literal(
                    Literal::Null
                    | Literal::Bool(_)
                    | Literal::Int(_)
                    | Literal::Float(_)
                    | Literal::BigInt(_)
                    | Literal::Decimal(_)
                    | Literal::Symbol(_)
                    | Literal::Char(_),
                )
                | PoolEntry::Function(_)
                | PoolEntry::Shape(_)
                | PoolEntry::Layout(_)) => {
                    panic!("property name is not a string constant: {other:?}")
                }
            };
            *sites.entry((name, slot)).or_default() += 1;
        }
        ip += info.len;
    }
    sites
}

fn ssa_sites(proto: &FunctionProto) -> Option<Sites> {
    let ssa = proto.ssa.get()?;
    let mut sites = Sites::new();
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let SsaOp::GetProperty { name, cs, .. } | SsaOp::SetProperty { name, cs, .. } =
                &inst.op
            {
                *sites.entry((name.to_string(), *cs)).or_default() += 1;
            }
        }
    }
    Some(sites)
}

fn index_order_slots(proto: &FunctionProto) -> Vec<u16> {
    let ssa = proto.ssa.get().expect("portable SSA");
    ssa.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter_map(|i| match &i.op {
            SsaOp::GetProperty { cs, .. } | SsaOp::SetProperty { cs, .. } => Some(*cs),
            SsaOp::ConstInt(_)
            | SsaOp::ConstFloat(_)
            | SsaOp::ConstBool(_)
            | SsaOp::ConstNull
            | SsaOp::ConstStr(_)
            | SsaOp::Binary { .. }
            | SsaOp::Unary { .. }
            | SsaOp::SelfCall { .. }
            | SsaOp::Call { .. }
            | SsaOp::AllocInstance { .. }
            | SsaOp::LoadGlobalIdx(_)
            | SsaOp::ArrayGetIndex { .. }
            | SsaOp::ArraySetIndex { .. }
            | SsaOp::ConstChar(_)
            | SsaOp::ConstBigInt(_)
            | SsaOp::ConstDecimal(_)
            | SsaOp::MakeEnumVariant { .. }
            | SsaOp::IntrinsicCall { .. }
            | SsaOp::LoadNativeGlobalIdx(_)
            | SsaOp::Try { .. }
            | SsaOp::PopTry
            | SsaOp::CatchParam { .. }
            | SsaOp::StoreGlobalIdx { .. }
            | SsaOp::MakeClosure { .. }
            | SsaOp::LoadCaptured { .. }
            | SsaOp::StoreCaptured { .. }
            | SsaOp::LoadUpvalue(_)
            | SsaOp::StoreUpvalue { .. }
            | SsaOp::CloseUpvalues { .. }
            | SsaOp::Cast { .. }
            | SsaOp::Convert { .. }
            | SsaOp::IsNull { .. }
            | SsaOp::Typeof { .. }
            | SsaOp::ToString { .. }
            | SsaOp::IsArray { .. }
            | SsaOp::GetEnumTag { .. }
            | SsaOp::ObjectKeys { .. }
            | SsaOp::BuildStr { .. }
            | SsaOp::BuildArray { .. }
            | SsaOp::BuildMap { .. }
            | SsaOp::BuildObject { .. }
            | SsaOp::GetIndex { .. }
            | SsaOp::SetIndex { .. }
            | SsaOp::ArrayLength { .. }
            | SsaOp::StrLength { .. }
            | SsaOp::BytesLength { .. }
            | SsaOp::MethodCall { .. }
            | SsaOp::CallNativeOp { .. }
            | SsaOp::ArrayPush { .. }
            | SsaOp::This
            | SsaOp::GetFixedField { .. }
            | SsaOp::SetFixedField { .. }
            | SsaOp::MakeClass { .. }
            | SsaOp::DeclareLayout { .. }
            | SsaOp::DefineMethod { .. }
            | SsaOp::GetSuper { .. }
            | SsaOp::LoadGlobal(_)
            | SsaOp::StoreGlobal { .. }
            | SsaOp::BuildTuple { .. }
            | SsaOp::BuildArraySpread { .. }
            | SsaOp::BuildObjectSpread { .. }
            | SsaOp::ObjectMerge { .. }
            | SsaOp::ObjectRest { .. }
            | SsaOp::GetPropertyMaybe { .. }
            | SsaOp::AssertNotNull { .. }
            | SsaOp::BindMethod { .. }
            | SsaOp::ArrayExtend { .. }
            | SsaOp::WrapSpread { .. }
            | SsaOp::Range { .. }
            | SsaOp::GetSymbol { .. }
            | SsaOp::IterCall { .. }
            | SsaOp::SuperCall { .. }
            | SsaOp::SuperMethodCall { .. }
            | SsaOp::ExtensionCall { .. }
            | SsaOp::CallSpread { .. }
            | SsaOp::LoadModule { .. }
            | SsaOp::ModuleSlot { .. }
            | SsaOp::StoreModuleSlot { .. }
            | SsaOp::Await { .. }
            | SsaOp::Spawn { .. }
            | SsaOp::Yield { .. }
            | SsaOp::Dispose { .. } => None,
        })
        .collect()
}

fn find<'a>(proto: &'a FunctionProto, name: &str) -> Option<&'a FunctionProto> {
    if proto.name.as_deref() == Some(name) {
        return Some(proto);
    }
    proto.chunk.constants.iter().find_map(|c| match c {
        PoolEntry::Function(f) => find(f, name),
        PoolEntry::Literal(_) | PoolEntry::Shape(_) | PoolEntry::Layout(_) => None,
    })
}

#[test]
fn portable_ssa_and_bytecode_share_cache_slots() {
    let module = varn_pipeline::stdlib_loader::compile_source(SOURCE, "ic_slots_agree.vn")
        .expect("compiles");
    let walk = find(&module, "walk").expect("`walk` is compiled");
    let slots = index_order_slots(walk);
    assert!(
        slots.iter().enumerate().any(|(i, cs)| *cs as usize != i),
        "emission order equals block order ({slots:?}): the test would prove nothing"
    );
    let from_ssa = ssa_sites(walk).expect("`walk` projects to portable SSA");
    let from_bytecode = bytecode_sites(walk);
    assert_eq!(
        from_bytecode.values().sum::<usize>(),
        6,
        "every property site is emitted"
    );
    assert_eq!(from_ssa, from_bytecode);
}
