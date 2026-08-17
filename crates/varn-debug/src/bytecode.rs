use varn_compiler::FunctionProto;
use varn_core::OpCode;
use varn_term::chalk::chalk;
use varn_term::terminal;
use varn_term::terminal::Section;
use varn_types::PoolEntry;

const R: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";

fn op_color(op: OpCode) -> &'static str {
    use OpCode::*;
    match op {
        Jump | JumpIfFalse | JumpIfTrue | Loop | Return | Throw | Try | PopTry => YELLOW,

        Call | CallSpread | CallSelf | CallMethod | InvokeVirtual | Spawn => MAGENTA,

        LoadConst | LoadInt | LoadIntZero | LoadIntOne | LoadIntMinusOne | LoadGlobal
        | LoadGlobalIdx | StoreGlobal | StoreGlobalIdx | DefineGlobal | DefineGlobalIdx
        | LoadModule | LoadModuleSlot | StoreModuleSlot | LoadUpvalue | StoreUpvalue => CYAN,

        MakeClass | MakeClosure | LoadStaticFn | BuildArray | BuildObject
        | BuildObjectWithShape | BuildStr | Method | DefineStatic | DefineGetter | DefineSetter
        | DefineStaticGetter | DefineStaticSetter | DeclareField | Inherit | BindMethod
        | MakeEnumVariant => BLUE,

        Add | Sub | Mul | Div | Mod | Pow | Eq | Neq | Lt | Lte | Gt | Gte | BitAnd | BitOr
        | BitXor | Shl | Shr | Ushr | In | Instanceof | AddInt | SubInt | MulInt | DivInt
        | ModInt | PowInt | LtInt | GtInt | LteInt | GteInt | EqInt | NeqInt | AddFloat
        | SubFloat | MulFloat | DivFloat | ModFloat | PowFloat | LtFloat | GtFloat | LteFloat
        | GteFloat | EqFloat | NeqFloat | StrConcat | StrSlice | AddImm | SubImm => GREEN,
        _ => "",
    }
}

fn const_hint(entry: &PoolEntry) -> String {
    use varn_types::chunk::Literal;
    match entry {
        PoolEntry::Literal(lit) => match lit {
            Literal::Null => format!("{DIM}null{R}"),
            Literal::Bool(b) => format!("{GREEN}{b}{R}"),
            Literal::Int(n) => format!("{GREEN}{n}{R}"),
            Literal::Float(f) => format!("{GREEN}{f}{R}"),
            Literal::Str(s) => format!("{GREEN}\"{s}\"{R}"),
            Literal::BigInt(n) => format!("{GREEN}{n}n{R}"),
            Literal::Decimal(d) => format!("{GREEN}{d}d{R}"),
            Literal::Char(c) => format!("{GREEN}'{c}'{R}"),
            Literal::Symbol(s) => format!("{DIM}Symbol({s:?}){R}"),
        },
        PoolEntry::Function(f) => {
            let fname = f.name.as_deref().unwrap_or("<anon>");
            format!("{BLUE}fn {fname}{R}(arity={})", f.arity)
        }
        PoolEntry::Shape(keys) => format!("{DIM}shape[{}]{R}", keys.len()),
    }
}

fn build_fn_index(proto: &FunctionProto) -> std::collections::HashMap<u16, String> {
    let mut map = std::collections::HashMap::new();
    for (i, entry) in proto.chunk.constants.iter().enumerate() {
        if let PoolEntry::Function(f) = entry {
            let name = f.name.as_deref().unwrap_or("<anon>").to_owned();
            map.insert(i as u16, name);
        }
    }
    map
}

pub fn debug_bytecode(proto: &FunctionProto, _flags: &crate::flags::DebugFlags) {
    Section::new("bytecode")
        .subtitle("...")
        .color(|c| c.yellow())
        .print();

    let mut total_words = 0;
    print_proto(proto, 0, &mut total_words);

    Section::new("bytecode")
        .subtitle(format!("{} bytecode words", total_words))
        .close();
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
    let state_size_flag =
        (proto.state_size != 0).then(|| format!("state_size={}", proto.state_size));
    let flags: Vec<&str> = [
        proto.is_async.then_some("async"),
        proto.is_generator.then_some("gen"),
        proto.has_this.then_some("has_this"),
        proto.has_rest.then_some("has_rest"),
    ]
    .into_iter()
    .flatten()
    .chain(state_size_flag.as_deref())
    .collect();
    let flags_str = if flags.is_empty() {
        String::new()
    } else {
        format!("  {DIM}[{}]{R}", flags.join(", "))
    };

    terminal::log(format!(
        "{indent}{} {} (arity: {}, regs: {}, upvalues: {}){flags_str}",
        chalk("fn").bold(),
        chalk(name).blue(),
        proto.arity,
        proto.register_count,
        proto.upvalue_count,
    ));

    if !proto.chunk.constants.is_empty() {
        terminal::log(format!(
            "{indent}  {DIM}constants ({}){R}",
            proto.chunk.constants.len()
        ));
        for (i, c) in proto.chunk.constants.iter().enumerate() {
            terminal::log(format!("{indent}  {DIM}[{:03}]{R} {}", i, const_hint(c)));
        }
    }
    terminal::log(format!(
        "{indent}  {DIM}code ({}) words{R}",
        proto.chunk.code.len()
    ));
    *total += proto.chunk.code.len();

    terminal::log(format!(
        "{indent}  {}",
        chalk(format!(
            "{:<4} │ {:<3} │ {:<20} │ Operands / Hint",
            "Off", "Lin", "Opcode"
        ))
        .dim()
    ));
    terminal::log(format!("{indent}  {}", "─".repeat(72)));

    let fn_index = build_fn_index(proto);

    let code = &proto.chunk.code;
    let mut pc = 0;
    while pc < code.len() {
        let start_pc = pc;
        let op_val = code[pc];
        pc += 1;
        let Some(op) = OpCode::from_u16(op_val) else {
            terminal::log(format!(
                "{indent}  {:04} │ ??? │ {:<20} │ raw={}",
                start_pc, "???", op_val
            ));
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
                format!("r{} = r{}", hi(op_val), hi(w1))
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
            | OpCode::WrapSpread
            | OpCode::ArrayLength
            | OpCode::StrLength
            | OpCode::GetEnumTag
            | OpCode::Await => {
                let w1 = w!();
                format!("r{} = r{}", hi(op_val), hi(w1))
            }
            OpCode::AssertNotNull => {
                let w1 = w!();
                format!("assert r{}", hi(w1))
            }
            OpCode::ArrayPop => {
                let w1 = w!();
                format!("r{} = r{}[].pop()", hi(op_val), hi(w1))
            }
            OpCode::ArrayPush => {
                let w1 = w!();
                format!("r{}[].push r{}", hi(op_val), hi(w1))
            }
            OpCode::LoadUpvalue | OpCode::StoreUpvalue | OpCode::CloseUpvalue => {
                let w1 = w!();
                format!("r{} uv={}", hi(w1), lo(w1))
            }
            OpCode::Return | OpCode::Throw | OpCode::Yield => {
                let w1 = w!();
                format!("r{}", lo(w1))
            }
            OpCode::StoreModuleSlot => {
                let slot_idx = w!();
                format!("r{} slot[{}]", hi(op_val), slot_idx)
            }
            OpCode::PopTry => String::new(),
            OpCode::Nop => String::new(),

            OpCode::CallNativeOp => {
                let cidx = w!();
                let argc = w!();
                format!("r{} = nativeop const[{}]({} args)", hi(op_val), cidx, argc)
            }

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
            | OpCode::ModInt
            | OpCode::PowInt
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
            | OpCode::NeqFloat => {
                let w1 = w!();
                format!("r{} = r{} op r{}", hi(op_val), hi(w1), lo(w1))
            }
            OpCode::StrSlice => {
                let w1 = w!();
                format!("r{} = r{}[r{}]", hi(op_val), hi(w1), lo(w1))
            }
            OpCode::In | OpCode::Instanceof => {
                let w1 = w!();
                let op_str = if matches!(op, OpCode::In) {
                    "in"
                } else {
                    "instanceof"
                };
                format!("r{} = r{} {} r{}", hi(op_val), hi(w1), lo(w1), op_str)
            }
            OpCode::ArrayExtend => {
                let w1 = w!();
                format!("r{}[].extend r{}", hi(op_val), hi(w1))
            }
            OpCode::ObjectKeys => {
                let w1 = w!();
                format!("r{} = keys(r{})", hi(op_val), hi(w1))
            }
            OpCode::ObjectMerge => {
                let w1 = w!();
                format!("r{} |= r{}", hi(w1), lo(w1))
            }
            OpCode::GetIndex | OpCode::ArrayGetIndex => {
                let w1 = w!();
                format!("r{} = r{}[r{}]", hi(op_val), hi(w1), lo(w1))
            }
            OpCode::SetIndex | OpCode::ArraySetIndex => {
                let w1 = w!();
                format!("r{}[r{}] = r{}", hi(op_val), hi(w1), lo(w1))
            }

            OpCode::LoadConst => {
                let idx = w!();
                if let Some(c) = proto.chunk.constants.get(idx as usize) {
                    hint = const_hint(c);
                }
                format!("r{} = const[{}]", hi(op_val), idx)
            }
            OpCode::LoadInt => {
                let val = w!() as i16;
                format!("r{} = {}", hi(op_val), val)
            }

            OpCode::LoadIntZero => format!("r{} = 0", hi(op_val)),
            OpCode::LoadIntOne => format!("r{} = 1", hi(op_val)),
            OpCode::LoadIntMinusOne => format!("r{} = -1", hi(op_val)),

            OpCode::LoadGlobal | OpCode::LoadGlobalIdx => {
                let idx = w!();
                if let Some(c) = proto.chunk.constants.get(idx as usize) {
                    hint = const_hint(c);
                }
                format!("r{} = global[{}]", hi(op_val), idx)
            }
            OpCode::StoreGlobal | OpCode::StoreGlobalIdx => {
                let w1 = w!();
                let idx = w!();
                if let Some(c) = proto.chunk.constants.get(idx as usize) {
                    hint = const_hint(c);
                }
                format!("global[{}] = r{}", idx, hi(w1))
            }
            OpCode::DefineGlobal | OpCode::DefineGlobalIdx => {
                let w1 = w!();
                let idx = w!();
                if let Some(c) = proto.chunk.constants.get(idx as usize) {
                    hint = const_hint(c);
                }
                format!("def global[{}] = r{}", idx, hi(w1))
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
            OpCode::CallSelf => {
                let w1 = w!();
                let w2 = w!();
                format!("r{} = callself ({} args @ r{})", hi(w1), hi(w2), lo(w2))
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
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!(
                    "r{} = r{}.{}({} args @ r{})",
                    hi(w1),
                    lo(w1),
                    name_idx,
                    hi(w3),
                    lo(w3)
                )
            }

            OpCode::GetProperty => {
                let w1 = w!();
                let name_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!(
                    "r{} = r{}.prop[{}] cs={}",
                    hi(op_val),
                    hi(w1),
                    name_idx,
                    lo(w1)
                )
            }
            OpCode::SetProperty => {
                let w1 = w!();
                let name_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!(
                    "r{}.prop[{}] = r{} cs={}",
                    hi(op_val),
                    name_idx,
                    hi(w1),
                    lo(w1)
                )
            }

            OpCode::GetPropertyMaybe => {
                let w1 = w!();
                let name_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{} = r{}.prop?[{}]", hi(op_val), hi(w1), name_idx)
            }
            OpCode::GetFixedField => {
                let w1 = w!();
                let idx = w!();
                format!("r{} = r{}.fixed[{}]", hi(op_val), hi(w1), idx)
            }
            OpCode::SetFixedField => {
                let w1 = w!();
                let idx = w!();
                format!("r{}.fixed[{}] = r{}", hi(op_val), idx, hi(w1))
            }
            OpCode::GetSuper => {
                let name_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = format!("{:?}", c);
                }
                format!("r{} = super.prop[{}]", hi(op_val), name_idx)
            }
            OpCode::GetSymbol => {
                let w1 = w!();
                let idx = w!();
                format!("r{} = r{}.sym[{}]", hi(op_val), hi(w1), idx)
            }
            OpCode::BindMethod => {
                let w1 = w!();
                let name_idx = w!();
                format!("r{} = r{}.bind[{}]", hi(w1), lo(w1), name_idx)
            }

            OpCode::BuildArray => {
                let w1 = w!();
                let w2 = w!();
                format!("r{} = [r{}..+{}]", hi(w1), lo(w1), hi(w2))
            }
            OpCode::BuildTuple => {
                let w1 = w!();
                let w2 = w!();
                format!("r{} = #[r{}..+{}]", hi(w1), lo(w1), hi(w2))
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
            OpCode::BuildRecord => {
                let w1 = w!();
                let shape_idx = w!();
                format!("r{} = record_shape[{}] vals@r{}", hi(w1), shape_idx, lo(w1))
            }
            OpCode::ObjectRest => {
                let w1 = w!();
                let w2 = w!();
                let skip = hi(w2) as usize;
                for _ in 0..skip {
                    let _ = w!();
                }
                format!("r{} = rest(r{}, skip={})", hi(w1), lo(w1), skip)
            }

            OpCode::MakeClosure => {
                let w1 = w!();
                let proto_idx = w!();
                let uv_count = lo(w1);

                if let Some(fn_name) = fn_index.get(&proto_idx) {
                    hint = format!("{BLUE}→ fn {fn_name}{R}");
                } else if let Some(c) = proto.chunk.constants.get(proto_idx as usize) {
                    hint = const_hint(c);
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
                    hint = const_hint(c);
                }
                let super_reg = hi(w1);
                let dest = hi(op_val);
                if super_reg != 0 {
                    format!(
                        "r{} = class[{}] extends r{} (raw={:04x} {:04x} {:04x})",
                        dest, name_idx, super_reg, op_val, w1, name_idx
                    )
                } else {
                    format!(
                        "r{} = class[{}] (raw={:04x} {:04x} {:04x})",
                        dest, name_idx, op_val, w1, name_idx
                    )
                }
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
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = const_hint(c);
                }
                format!("r{}[{}] = r{}", hi(w1), name_idx, lo(w1))
            }
            OpCode::DeclareField => {
                let w1 = w!();
                let name_idx = w!();
                if let Some(c) = proto.chunk.constants.get(name_idx as usize) {
                    hint = const_hint(c);
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
                    hint = const_hint(c);
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
                let w3 = w!();
                let _w4 = w!();
                let arg_count = hi(w3);
                if let Some(c) = proto.chunk.constants.get(fn_idx as usize) {
                    hint = const_hint(c);
                }
                format!(
                    "r{} = runtime[{}]({} args @ r{})",
                    hi(w1),
                    fn_idx,
                    arg_count,
                    lo(w3)
                )
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

            OpCode::Intrinsic => {
                let dest = hi(op_val);
                let w1 = w!();
                let wire_byte = (w1 >> 8) as u8;
                let arg_count = (w1 & 0xFF) as usize;
                format!("r{dest} = intrinsic(0x{wire_byte:02x}, {arg_count} args)")
            }

            OpCode::IntrinsicDirect => {
                let dest = hi(op_val);
                let w1 = w!();
                let src = (w1 >> 8) as u8;
                let wire_byte = (w1 & 0xFF) as u8;
                format!("r{dest} = intrinsic_direct(0x{wire_byte:02x}, r{src})")
            }

            OpCode::LoadStaticFn => {
                let dest = hi(op_val);
                let proto_idx = w!();

                if let Some(fn_name) = fn_index.get(&proto_idx) {
                    hint = format!("{BLUE}→ fn {fn_name}{R}");
                } else if let Some(c) = proto.chunk.constants.get(proto_idx as usize) {
                    hint = const_hint(c);
                }
                format!("r{dest} = static_fn[{proto_idx}]")
            }
        };

        let color = op_color(op);
        let op_name = format!("{color}{:<20}{R}", format!("{:?}", op));
        terminal::log(format!(
            "{indent}  {:04} │ {:>3} │ {} │ {}{}",
            start_pc,
            line,
            op_name,
            operands,
            if hint.is_empty() {
                String::new()
            } else {
                format!("  {DIM};{R} {}", hint)
            },
        ));
    }

    crate::loop_diagnostics::print_loop_diagnostics(
        &proto.chunk.code,
        &proto.chunk.constants,
        &indent,
    );
    terminal::blank();

    for entry in &proto.chunk.constants {
        if let varn_types::PoolEntry::Function(nested) = entry {
            print_proto(nested, depth + 1, total);
        }
    }
}
