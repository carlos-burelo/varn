use varn_compiler::FunctionProto;
use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::term::terminal::Section;
use varn_core::OpCode;
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
        | LoadGlobalIdx | LoadNativeGlobalIdx | StoreGlobal | StoreGlobalIdx | DefineGlobal
        | DefineGlobalIdx | LoadModule | LoadModuleSlot | StoreModuleSlot | LoadUpvalue
        | StoreUpvalue => CYAN,

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

/// Global keys are `<module path>::<symbol>`, and the module part is an
/// absolute path that buries the symbol the reader came for. Keep the module's
/// file name so cross-module globals stay distinguishable, drop the rest.
/// A string that is not a qualified key is left exactly as it is.
fn short_global_key(s: &str) -> &str {
    let Some((module, symbol)) = s.rsplit_once("::") else {
        return s;
    };
    let is_sep = |c: char| c == '/' || c == '\\';
    if !module.contains(is_sep) {
        return s;
    }
    let file = module
        .rsplit(is_sep)
        .next()
        .filter(|f| !f.is_empty())
        .unwrap_or(module);
    // `s` is contiguous, so the tail starting at the file name covers both.
    &s[s.len() - (file.len() + 2 + symbol.len())..]
}

fn const_hint(entry: &PoolEntry) -> String {
    use varn_types::chunk::Literal;
    match entry {
        PoolEntry::Literal(lit) => match lit {
            Literal::Null => format!("{DIM}null{R}"),
            Literal::Bool(b) => format!("{GREEN}{b}{R}"),
            Literal::Int(n) => format!("{GREEN}{n}{R}"),
            Literal::Float(f) => format!("{GREEN}{f}{R}"),
            Literal::Str(s) => format!("{GREEN}\"{}\"{R}", short_global_key(s)),
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

pub fn debug_bytecode(proto: &FunctionProto, flags: &crate::flags::DebugFlags) {
    Section::new("bytecode")
        .subtitle("...")
        .color(|c| c.yellow())
        .print();

    let mut total_words = 0;
    print_proto(proto, 0, &mut total_words, flags);

    Section::new("bytecode")
        .subtitle(format!("{} bytecode words", total_words))
        .close();
}

fn print_proto(
    proto: &FunctionProto,
    depth: usize,
    total: &mut usize,
    flags: &crate::flags::DebugFlags,
) {
    let name = proto.name.as_deref().unwrap_or("<anonymous>");
    let matches_filter = flags
        .fn_filter
        .as_ref()
        .is_none_or(|needle| name.contains(needle.as_str()));

    let indent = "  ".repeat(depth);
    let state_size_flag =
        (proto.state_size != 0).then(|| format!("state_size={}", proto.state_size));
    let proto_flags: Vec<&str> = [
        proto.is_async.then_some("async"),
        proto.is_generator.then_some("gen"),
        proto.has_this.then_some("has_this"),
        proto.has_rest.then_some("has_rest"),
    ]
    .into_iter()
    .flatten()
    .chain(state_size_flag.as_deref())
    .collect();
    let flags_str = if proto_flags.is_empty() {
        String::new()
    } else {
        format!("  {DIM}[{}]{R}", proto_flags.join(", "))
    };

    if matches_filter {
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

        for instr in varn_types::bytecode::disasm::instructions(&proto.chunk) {
            let (name, color) = match instr.op {
                Some(op) => (format!("{op:?}"), op_color(op)),
                None => ("???".to_owned(), ""),
            };
            let hint = instr
                .constants
                .iter()
                .filter_map(|&i| proto.chunk.constants.get(i).map(const_hint))
                .collect::<Vec<_>>()
                .join(", ");
            terminal::log(format!(
                "{indent}  {:04} │ {:>3} │ {color}{name:<20}{R} │ {}{}",
                instr.offset,
                instr.line,
                instr.text,
                if hint.is_empty() {
                    String::new()
                } else {
                    format!("  {DIM};{R} {hint}")
                },
            ));
        }

        crate::loop_diagnostics::print_loop_diagnostics(
            &proto.chunk.code,
            &proto.chunk.constants,
            &indent,
        );
        terminal::blank();
    }

    for entry in &proto.chunk.constants {
        if let varn_types::PoolEntry::Function(nested) = entry {
            print_proto(
                nested,
                if matches_filter { depth + 1 } else { depth },
                total,
                flags,
            );
        }
    }
}
