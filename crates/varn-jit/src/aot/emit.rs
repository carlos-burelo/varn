use super::RtHelpers;
use cranelift_codegen::ir::{types, InstBuilder, StackSlotData, StackSlotKind, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{DataId, Linkage, Module};
use cranelift_object::ObjectModule;
use rustc_hash::FxHashMap as HashMap;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::{Literal, PoolEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Null,
    Int,
    Bool,
    Str,
}

struct Regs {
    vars: Vec<(Variable, Variable)>,
    kinds: Vec<Option<Kind>>,
}

impl Regs {
    fn define(&mut self, builder: &mut FunctionBuilder, r: usize, v: Value, kind: Kind) {
        builder.def_var(self.vars[r].0, v);
        self.kinds[r] = Some(kind);
    }

    fn define_str(&mut self, builder: &mut FunctionBuilder, r: usize, ptr: Value, len: Value) {
        builder.def_var(self.vars[r].0, ptr);
        builder.def_var(self.vars[r].1, len);
        self.kinds[r] = Some(Kind::Str);
    }

    fn operand(
        &self,
        builder: &mut FunctionBuilder,
        r: usize,
        want: Kind,
    ) -> Result<Value, String> {
        match self.kinds[r] {
            Some(kind) if kind == want => Ok(builder.use_var(self.vars[r].0)),
            other => Err(format!("aot: expected a {want:?} in r{r}, found {other:?}")),
        }
    }
}

pub(super) fn emit_module_body(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    proto: &varn_types::FunctionProto,
    rt: &RtHelpers,
) -> Result<(), String> {
    let code = &proto.chunk.code;
    let constants = &proto.chunk.constants;
    let nregs = proto.register_count as usize;

    let mut regs = Regs {
        vars: Vec::with_capacity(nregs),
        kinds: vec![None; nregs],
    };
    for _ in 0..nregs {
        let pair = (
            builder.declare_var(types::I64),
            builder.declare_var(types::I64),
        );
        let z = builder.ins().iconst(types::I64, 0);
        builder.def_var(pair.0, z);
        builder.def_var(pair.1, z);
        regs.vars.push(pair);
    }

    let print = varn_core::abi::native_global_index("print");
    let mut natives: HashMap<usize, u32> = HashMap::default();
    let mut data_ids: HashMap<usize, DataId> = HashMap::default();

    let mut ip = 0usize;
    while ip < code.len() {
        let info = decode(code, ip, constants)
            .ok_or_else(|| format!("aot: undecodable instruction at word {ip}"))?;
        let w0 = code[ip];
        let first_reg = (w0 >> 8) as usize;
        let op = OpCode::from_u8((w0 & 0xFF) as u8)
            .ok_or_else(|| format!("aot: unknown opcode at word {ip}"))?;
        if let Some(def) = info.def {
            natives.remove(&(def as usize));
            regs.kinds[def as usize] = None;
        }

        match op {
            OpCode::LoadInt => {
                let v = builder.ins().iconst(types::I64, code[ip + 1] as i16 as i64);
                regs.define(builder, first_reg, v, Kind::Int);
            }
            OpCode::LoadIntZero | OpCode::LoadIntOne => {
                let n = i64::from(op == OpCode::LoadIntOne);
                let v = builder.ins().iconst(types::I64, n);
                regs.define(builder, first_reg, v, Kind::Int);
            }
            OpCode::LoadTrue | OpCode::LoadFalse => {
                let v = builder
                    .ins()
                    .iconst(types::I64, i64::from(op == OpCode::LoadTrue));
                regs.define(builder, first_reg, v, Kind::Bool);
            }
            OpCode::LoadNull => {
                let v = builder.ins().iconst(types::I64, 0);
                regs.define(builder, first_reg, v, Kind::Null);
            }
            OpCode::LoadConst => {
                let cidx = code[ip + 1] as usize;
                match constants.get(cidx) {
                    Some(PoolEntry::Literal(Literal::Str(s))) => {
                        let data_id = string_data(module, &mut data_ids, cidx, s.as_bytes())?;
                        let gv = module.declare_data_in_func(data_id, builder.func);
                        let ptr = builder.ins().symbol_value(types::I64, gv);
                        let len = builder.ins().iconst(types::I64, s.len() as i64);
                        regs.define_str(builder, first_reg, ptr, len);
                    }
                    Some(PoolEntry::Literal(Literal::Int(n))) => {
                        let v = builder.ins().iconst(types::I64, *n);
                        regs.define(builder, first_reg, v, Kind::Int);
                    }
                    Some(PoolEntry::Literal(Literal::Bool(b))) => {
                        let v = builder.ins().iconst(types::I64, i64::from(*b));
                        regs.define(builder, first_reg, v, Kind::Bool);
                    }
                    Some(other) => return Err(format!("aot: constant {other:?} is not supported")),
                    None => return Err(format!("aot: constant {cidx} is out of the pool")),
                }
            }
            OpCode::LoadNativeGlobalIdx => {
                natives.insert(first_reg, u32::from(code[ip + 1]));
            }
            OpCode::Move => {
                let src = (code[ip + 1] >> 8) as usize;
                if let Some(&idx) = natives.get(&src) {
                    natives.insert(first_reg, idx);
                }
                let val = builder.use_var(regs.vars[src].0);
                let meta = builder.use_var(regs.vars[src].1);
                builder.def_var(regs.vars[first_reg].0, val);
                builder.def_var(regs.vars[first_reg].1, meta);
                regs.kinds[first_reg] = regs.kinds[src];
            }
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Mod => {
                let w1 = code[ip + 1];
                let lhs = regs.operand(builder, (w1 >> 8) as usize, Kind::Int)?;
                let rhs = regs.operand(builder, (w1 & 0xFF) as usize, Kind::Int)?;
                let res = if op == OpCode::Add {
                    builder.ins().iadd(lhs, rhs)
                } else if op == OpCode::Sub {
                    builder.ins().isub(lhs, rhs)
                } else if op == OpCode::Mul {
                    builder.ins().imul(lhs, rhs)
                } else {
                    builder.ins().srem(lhs, rhs)
                };
                regs.define(builder, first_reg, res, Kind::Int);
            }
            OpCode::AddImm => {
                let w1 = code[ip + 1];
                let src = regs.operand(builder, (w1 >> 8) as usize, Kind::Int)?;
                let res = builder.ins().iadd_imm_s(src, (w1 & 0xFF) as i8 as i64);
                regs.define(builder, first_reg, res, Kind::Int);
            }
            OpCode::StrConcat => {
                let w1 = code[ip + 1];
                let (l, r) = ((w1 >> 8) as usize, (w1 & 0xFF) as usize);
                let a_ptr = regs.operand(builder, l, Kind::Str)?;
                let b_ptr = regs.operand(builder, r, Kind::Str)?;
                let a_len = builder.use_var(regs.vars[l].1);
                let b_len = builder.use_var(regs.vars[r].1);
                let slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    16,
                    3,
                ));
                let out = builder.ins().stack_addr(types::I64, slot, 0);
                let fref = module.declare_func_in_func(rt.str_concat, builder.func);
                builder.ins().call(fref, &[a_ptr, a_len, b_ptr, b_len, out]);
                let ptr = builder.ins().stack_load(types::I64, types::I64, slot, 0);
                let len = builder.ins().stack_load(types::I64, types::I64, slot, 8);
                regs.define_str(builder, first_reg, ptr, len);
            }
            OpCode::Call => {
                let callee = (code[ip + 1] & 0xFF) as usize;
                let w2 = code[ip + 2];
                let argc = (w2 >> 8) as usize;
                let receiver = (w2 & 0xFF) as usize;
                if natives.get(&callee).copied() != print || argc != 2 {
                    return Err("aot: only `print(value)` calls are supported".to_owned());
                }
                emit_print(builder, module, rt, &regs, receiver + 1)?;
            }
            OpCode::Return => break,
            OpCode::LoadGlobal
            | OpCode::StoreGlobal
            | OpCode::DefineGlobal
            | OpCode::DefineGlobalIdx
            | OpCode::LoadGlobalIdx
            | OpCode::StoreGlobalIdx
            | OpCode::LoadUpvalue
            | OpCode::StoreUpvalue
            | OpCode::CloseUpvalue
            | OpCode::Div
            | OpCode::Pow
            | OpCode::Negate
            | OpCode::Not
            | OpCode::ToString
            | OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr
            | OpCode::Jump
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue
            | OpCode::Loop
            | OpCode::CallMethod
            | OpCode::InvokeVirtual
            | OpCode::CallSpread
            | OpCode::BuildArray
            | OpCode::BuildTuple
            | OpCode::BuildObject
            | OpCode::BuildObjectWithShape
            | OpCode::BuildRecord
            | OpCode::GetIndex
            | OpCode::SetIndex
            | OpCode::ObjectRest
            | OpCode::ObjectKeys
            | OpCode::ObjectMerge
            | OpCode::GetProperty
            | OpCode::GetPropertyMaybe
            | OpCode::SetProperty
            | OpCode::GetFixedField
            | OpCode::SetFixedField
            | OpCode::GetSuper
            | OpCode::GetSymbol
            | OpCode::MakeClosure
            | OpCode::MakeClass
            | OpCode::Inherit
            | OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter
            | OpCode::DeclareLayout
            | OpCode::AllocInstance
            | OpCode::BindMethod
            | OpCode::Typeof
            | OpCode::Instanceof
            | OpCode::In
            | OpCode::IsNull
            | OpCode::IsArray
            | OpCode::AssertNotNull
            | OpCode::StrLength
            | OpCode::StrSlice
            | OpCode::ArrayLength
            | OpCode::ArrayPush
            | OpCode::ArrayPop
            | OpCode::ArrayExtend
            | OpCode::WrapSpread
            | OpCode::MakeEnumVariant
            | OpCode::GetEnumTag
            | OpCode::Await
            | OpCode::Spawn
            | OpCode::Yield
            | OpCode::Try
            | OpCode::Throw
            | OpCode::PopTry
            | OpCode::LoadModule
            | OpCode::LoadModuleSlot
            | OpCode::StoreModuleSlot
            | OpCode::InvokeRuntimeStatic
            | OpCode::SubImm
            | OpCode::BuildStr
            | OpCode::LoadIntMinusOne
            | OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::DivInt
            | OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt
            | OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ModFloat
            | OpCode::PowFloat
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::EqStr
            | OpCode::NeqStr
            | OpCode::ModInt
            | OpCode::PowInt
            | OpCode::Intrinsic
            | OpCode::LoadStaticFn
            | OpCode::CallSelf
            | OpCode::Nop
            | OpCode::ArrayGetIndex
            | OpCode::ArraySetIndex
            | OpCode::BytesGetIndex
            | OpCode::BytesSetIndex
            | OpCode::CallNativeOp
            | OpCode::IntrinsicDirect
            | OpCode::BuildMap
            | OpCode::MapGetIndex
            | OpCode::MapSetIndex
            | OpCode::Convert
            | OpCode::BytesLength => {
                return Err(format!(
                    "aot: {op:?} is not supported by the native backend"
                ));
            }
        }

        ip += info.len;
    }

    Ok(())
}

fn emit_print(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    rt: &RtHelpers,
    regs: &Regs,
    arg: usize,
) -> Result<(), String> {
    let (helper, args) = match regs.kinds[arg] {
        Some(Kind::Str) => (
            rt.print_str,
            vec![
                builder.use_var(regs.vars[arg].0),
                builder.use_var(regs.vars[arg].1),
            ],
        ),
        Some(Kind::Int) => (rt.print_int, vec![builder.use_var(regs.vars[arg].0)]),
        Some(Kind::Bool) => (rt.print_bool, vec![builder.use_var(regs.vars[arg].0)]),
        Some(Kind::Null) => (rt.print_null, Vec::new()),
        None => return Err(format!("aot: print of r{arg}, whose type is unknown")),
    };
    let fref = module.declare_func_in_func(helper, builder.func);
    builder.ins().call(fref, &args);
    Ok(())
}

fn string_data(
    module: &mut ObjectModule,
    cache: &mut HashMap<usize, DataId>,
    idx: usize,
    bytes: &[u8],
) -> Result<DataId, String> {
    if let Some(&id) = cache.get(&idx) {
        return Ok(id);
    }
    let data_id = module
        .declare_data(&format!(".str.{idx}"), Linkage::Local, false, false)
        .map_err(|e| format!("aot: declare data: {e}"))?;
    let mut desc = cranelift_module::DataDescription::new();
    desc.define(bytes.to_vec().into_boxed_slice());
    module
        .define_data(data_id, &desc)
        .map_err(|e| format!("aot: define data: {e}"))?;
    cache.insert(idx, data_id);
    Ok(data_id)
}
