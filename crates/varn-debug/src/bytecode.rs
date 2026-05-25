use crate::colors::{BLUE, BOLD, C_BYTECODE, DIM, RESET};
use varn_compiler::FunctionProto;
use varn_core::OpCode;

pub fn debug_bytecode(proto: &FunctionProto, _flags: &crate::flags::DebugFlags) {
    use crate::colors::{footer, header};
    header(C_BYTECODE, "bytecode", "...");

    let mut total_words = 0;
    print_proto(proto, 0, &mut total_words);

    footer(C_BYTECODE, &format!("{} bytecode words", total_words));
}

fn hi(w: u16) -> usize {
    (w >> 8) as usize
}
fn lo(w: u16) -> usize {
    (w & 0xFF) as usize
}

fn print_proto(proto: &FunctionProto, depth: usize, total: &mut usize) {
    let indent = "  ".repeat(depth);
    let name = proto.name.as_deref().unwrap_or("<anonymous>");

    eprintln!(
        "{indent}{BOLD}fn{RESET} {BLUE}{}{RESET} (arity: {}, regs: {}, upvalues: {})",
        name,
        proto.arity,
        proto.register_count,
        proto.upvalue_count,
        indent = indent,
        BOLD = BOLD,
        RESET = RESET,
        BLUE = BLUE
    );

    if !proto.chunk.constants.is_empty() {
        eprintln!("{indent}  constants ({})", proto.chunk.constants.len());
        for (i, c) in proto.chunk.constants.iter().enumerate() {
            eprintln!("{indent}  [{:03}] {:?}", i, c);
        }
    }
    eprintln!("{indent}  code ({}) words", proto.chunk.code.len());
    *total += proto.chunk.code.len();

    eprintln!(
        "{indent}  {DIM}{:<4} │ {:<3} │ {:<20} │ Operands / Hint{RESET}",
        "Off", "Lin", "Opcode"
    );
    eprintln!("{indent}  {}", "─".repeat(72));

    let code = &proto.chunk.code;
    let mut pc = 0;
    while pc < code.len() {
        let start_pc = pc;
        let op_val = code[pc];
        pc += 1;
        let Some(op) = OpCode::from_u16(op_val) else {
            eprintln!(
                "{indent}  {:04} │ ??? │ {:<20} │ raw={}",
                start_pc, "???", op_val
            );
            continue;
        };
        let line = proto.chunk.lines.get_line(start_pc);
        let mut hint = String::new();

        macro_rules! w {
            () => {{
                let v = code.get(pc).copied().unwrap_or(0);
                pc += 1;
                v
            }};
        }

        let operands = match op {
            OpCode::Move => {
                let w1 = w!();
                format!("r{} = r{}", hi(w1), lo(w1))
            }

            OpCode::LoadNull | OpCode::LoadTrue | OpCode::LoadFalse => {
                format!("r{}", hi(op_val))
            }
            OpCode::Negate
            | OpCode::Not
            | OpCode::ToString
            | OpCode::IsNull
            | OpCode::IsArray
            | OpCode::Typeof
            | OpCode::AssertNotNull
            | OpCode::WrapSpread
            | OpCode::ArrayLength
            | OpCode::ArrayPop
            | OpCode::StrLength
            | OpCode::GetEnumTag
            | OpCode::Await => {
                let w1 = w!();
                format!("r{} = r{}", hi(w1), lo(w1))
            }
            OpCode::ArrayPush => {
                let w1 = w!();
                format!("r{}[].push r{}", hi(w1), lo(w1))
            }
            OpCode::LoadUpvalue | OpCode::StoreUpvalue | OpCode::CloseUpvalue => {
                let w1 = w!();
                format!("r{} uv={}", hi(w1), lo(w1))
            }
            OpCode::Return | OpCode::Throw | OpCode::StoreModuleSlot | OpCode::Yield => {
                let w1 = w!();
                format!("r{}", lo(w1))
            }
            OpCode::PopTry => String::new(),
            OpCode::Nop => String::new(),

            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr
            | OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::StrConcat
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
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat => {
                let w1 = w!();
                let w2 = w!();
                format!("r{} = r{} op r{}", hi(w1), lo(w1), hi(w2))
            }
            OpCode::StrSlice => {
                let w1 = w!();
                let w2 = w!();
                format!("r{} = r{}[r{}..r{}]", hi(w1), lo(w1), hi(w2), lo(w2))
            }
            OpCode::In | OpCode::Instanceof => {
                let w1 = w!();
                let w2 = w!();
                format!("r{} = r{} in r{}", hi(w1), lo(w1), hi(w2))
            }
            OpCode::ArrayExtend => {
                let w1 = w!();
                let w2 = w!();
                format!("r{}[].extend r{}", hi(w1), hi(w2))
            }
            OpCode::ObjectKeys => {
                let w1 = w!();
                let w2 = w!();
                format!("r{} = keys(r{})", hi(w1), lo(w2))
            }
            OpCode::ObjectMerge => {
                let w1 = w!();
                format!("r{} |= r{}", hi(w1), lo(w1))
            }
            OpCode::GetIndex | OpCode::SetIndex => {
                let w1 = w!();
                let w2 = w!();
                format!("r{}[r{}] = r{}", hi(w1), lo(w1), hi(w2))
            }

            OpCode::LoadConst => {
                let w1 = w!();
                let idx = w!();
                if let Some(c) = proto.chunk.constants.get(idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{} = const[{}]", hi(w1), idx)
            }
            OpCode::LoadInt => {
                let w1 = w!();
                let val = w!() as i16;
                format!("r{} = {}", hi(w1), val)
            }

            OpCode::LoadIntZero => format!("r{} = 0", hi(op_val)),
            OpCode::LoadIntOne => format!("r{} = 1", hi(op_val)),
            OpCode::LoadIntMinusOne => format!("r{} = -1", hi(op_val)),

            OpCode::LoadGlobal | OpCode::LoadGlobalIdx => {
                let w1 = w!();
                let idx = w!();
                if let Some(c) = proto.chunk.constants.get(idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{} = global[{}]", hi(w1), idx)
            }
            OpCode::StoreGlobal | OpCode::StoreGlobalIdx => {
                let w1 = w!();
                let idx = w!();
                if let Some(c) = proto.chunk.constants.get(idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("global[{}] = r{}", idx, lo(w1))
            }
            OpCode::DefineGlobal | OpCode::DefineGlobalIdx => {
                let w1 = w!();
                let idx = w!();
                if let Some(c) = proto.chunk.constants.get(idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("def global[{}] = r{}", idx, lo(w1))
            }

            OpCode::Jump | OpCode::Loop => {
                let hi = w!() as u32;
                let lo = w!() as u32;
                let offset = ((hi << 16) | lo) as usize;
                format!("→ +{}", offset)
            }
            OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                let cond_reg = hi(op_val);
                let hi = w!() as u32;
                let lo = w!() as u32;
                let offset = ((hi << 16) | lo) as usize;
                format!("r{} → +{}", cond_reg, offset)
            }

            OpCode::Call | OpCode::CallSpread => {
                let w1 = w!();
                let w2 = w!();
                format!(
                    "r{} = call r{}({} args @ r{})",
                    hi(w1),
                    lo(w1),
                    hi(w2),
                    lo(w2)
                )
            }
            OpCode::InvokeVirtual => {
                let w1 = w!();
                let name_idx = w!();
                let w3 = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!(
                    "r{} = r{}.vtable[{}]({} args)",
                    hi(w1),
                    lo(w1),
                    name_idx,
                    hi(w3)
                )
            }

            OpCode::CallMethod => {
                let w1 = w!();
                let name_idx = w!();
                let w3 = w!();
                let w4 = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!(
                    "r{} = r{}.{}({} args @ r{})",
                    hi(w1),
                    lo(w1),
                    name_idx,
                    hi(w3),
                    hi(w4)
                )
            }

            OpCode::GetProperty => {
                let w1 = w!();
                let name_idx = w!();
                let cs_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{} = r{}.prop[{}] cs={}", hi(w1), lo(w1), name_idx, cs_idx)
            }
            OpCode::SetProperty => {
                let w1 = w!();
                let name_idx = w!();
                let cs_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{}.prop[{}] = r{} cs={}", hi(w1), name_idx, lo(w1), cs_idx)
            }

            OpCode::GetPropertyMaybe => {
                let w1 = w!();
                let name_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{} = r{}.prop?[{}]", hi(w1), lo(w1), name_idx)
            }
            OpCode::GetFixedField | OpCode::SetFixedField => {
                let w1 = w!();
                let idx = w!();
                format!("r{} fixed[{}] r{}", hi(w1), idx, lo(w1))
            }
            OpCode::GetSuper => {
                let w1 = w!();
                let name_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{} = super.prop[{}]", hi(w1), name_idx)
            }
            OpCode::GetSymbol => {
                let w1 = w!();
                let idx = w!();
                format!("r{} = sym[{}]", hi(w1), idx)
            }
            OpCode::BindMethod => {
                let w1 = w!();
                let name_idx = w!();
                format!("r{} = r{}.bind[{}]", hi(w1), lo(w1), name_idx)
            }

            OpCode::BuildArray => {
                let w1 = w!();
                let w2 = w!();
                format!("r{} = [r{}..+{}]", hi(w1), lo(w1), lo(w2))
            }
            OpCode::BuildObject => {
                let w1 = w!();
                let count = lo(w1);
                let dest = hi(w1);
                let s = format!("r{} = {{{} pairs}}", dest, count);
                for _ in 0..count {
                    let _ = w!();
                    let _ = w!();
                }
                s
            }
            OpCode::BuildObjectWithShape => {
                let w1 = w!();
                let shape_idx = w!();
                format!("r{} = shape[{}] vals@r{}", hi(w1), shape_idx, lo(w1))
            }
            OpCode::ObjectRest => {
                let w1 = w!();
                let w2 = w!();
                let skip = lo(w2) as usize;
                for _ in 0..skip {
                    let _ = w!();
                }
                format!("r{} = rest(r{}, skip={})", hi(w1), lo(w1), skip)
            }

            OpCode::MakeClosure => {
                let w1 = w!();
                let proto_idx = w!();
                let uv_count = lo(w1);
                if let Some(c) = proto.chunk.constants.get(proto_idx as usize) {
                    hint = format!("{:?}", c);
                }
                let dest = hi(w1);
                for _ in 0..uv_count {
                    let _ = w!();
                }
                format!("r{} = closure[{}] uvs={}", dest, proto_idx, uv_count)
            }

            OpCode::MakeClass => {
                let w1 = w!();
                let name_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{} = class[{}]", hi(w1), name_idx)
            }
            OpCode::Inherit => {
                let w1 = w!();
                format!("r{} extends r{}", hi(w1), lo(w1))
            }
            OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter => {
                let w1 = w!();
                let name_idx = w!();
                let w3 = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{}[{}] = r{}", hi(w1), name_idx, hi(w3))
            }
            OpCode::DeclareField => {
                let w1 = w!();
                let name_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{} field[{}]", hi(w1), name_idx)
            }

            OpCode::MakeEnumVariant => {
                let w1 = w!();
                let w2 = w!();
                format!("r{} = enum variant tag={}", hi(w1), lo(w2))
            }

            OpCode::Try => {
                let err_w = w!();
                let err_reg = hi(err_w);
                let hi_off = w!() as u32;
                let lo_off = w!() as u32;
                let catch_offset = ((hi_off << 16) | lo_off) as usize;
                format!("err=r{} catch → +{}", err_reg, catch_offset)
            }

            OpCode::LoadModule => {
                let dest = hi(op_val);
                let const_idx = w!();
                if let Some(c) = proto.chunk.constants.get(const_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{} = load module[{}]", dest, const_idx)
            }
            OpCode::LoadModuleSlot => {
                let dest = hi(op_val);
                let w1 = w!();
                let src = hi(w1);
                let slot_idx = w!();
                format!("r{} = load module slot r{}[{}]", dest, src, slot_idx)
            }

            OpCode::Spawn => {
                let w1 = w!();
                let w2 = w!();
                format!("r{} = spawn r{}({} args)", hi(w1), lo(w1), hi(w2))
            }

            OpCode::InvokeRuntimeStatic => {
                let w1 = w!();
                let fn_idx = w!();
                let arg_count = lo(w1);
                for _ in 0..arg_count {
                    let _ = w!();
                }
                format!("r{} = runtime[{}]({} args)", hi(w1), fn_idx, arg_count)
            }

            OpCode::AddImm | OpCode::SubImm => {
                let dest = hi(op_val);
                let w1 = w!();
                let src = hi(w1);
                let imm = lo(w1) as i8;
                let sign = if matches!(op, OpCode::SubImm) {
                    "-"
                } else {
                    "+"
                };
                format!("r{dest} = r{src} {sign} {imm}")
            }

            OpCode::BuildStr => {
                let dest = hi(op_val);
                let w1 = w!();
                let count = hi(w1) as usize;
                let mut reg_list = Vec::with_capacity(count);
                for _ in 0..count {
                    reg_list.push(format!("r{}", hi(w!())));
                }
                format!("r{dest} = concat({})", reg_list.join(", "))
            }
        };

        eprintln!(
            "{indent}  {:04} │ {:>3} │ {BOLD}{:<20}{RESET} │ {}{}",
            start_pc,
            line,
            format!("{:?}", op),
            operands,
            if hint.is_empty() {
                String::new()
            } else {
                format!("  ; {}", hint)
            },
            indent = indent,
            BOLD = BOLD,
            RESET = RESET,
        );
    }

    for entry in &proto.chunk.constants {
        if let varn_types::PoolEntry::Function(nested) = entry {
            eprintln!();
            print_proto(nested, depth + 1, total);
        }
    }

    eprintln!();
}
