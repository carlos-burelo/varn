use std::fmt::Write;

use crate::hir::{HirBinOp, HirType, HirUnOp};

use super::ir::{Block, InstKind, SsaFunc, Terminator, Value};

pub fn dump(func: &SsaFunc) -> String {
    let mut out = String::new();
    let mut flags = Vec::new();
    if func.is_async {
        flags.push("async");
    }
    if func.is_generator {
        flags.push("generator");
    }
    if flags.is_empty() {
        let _ = writeln!(out, "fn {}:", func.name);
    } else {
        let _ = writeln!(out, "fn {}: [{}]", func.name, flags.join(", "));
    }
    for (i, block) in func.blocks.iter().enumerate() {
        dump_block(&mut out, func, i as u32, block);
    }
    out
}

fn dump_block(out: &mut String, func: &SsaFunc, id: u32, block: &Block) {
    let params = block
        .params
        .iter()
        .map(|v| format!("{}: {}", val(*v), ty(func.value_ty(*v))))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(out, "  b{id}({params}):");
    for inst in &block.insts {
        // Non-Dynamic result types are printed so typed-value coverage is
        // verifiable from the dump.
        let lhs = match inst.dest {
            Some(v) => match func.value_ty(v) {
                HirType::Dynamic => format!("{} = ", val(v)),
                t => format!("{}: {} = ", val(v), ty(t)),
            },
            None => String::new(),
        };
        let _ = writeln!(out, "    {lhs}{}", inst_kind(&inst.kind));
    }
    let _ = writeln!(out, "    {}", terminator(&block.term));
}

fn inst_kind(kind: &InstKind) -> String {
    match kind {
        InstKind::ConstInt(n) => format!("int {n}"),
        InstKind::ConstFloat(n) => format!("float {n}"),
        InstKind::ConstBool(b) => format!("bool {b}"),
        InstKind::ConstStr(s) => format!("str {s:?}"),
        InstKind::ConstChar(c) => format!("char {c:?}"),
        InstKind::ConstDecimal(d) => format!("decimal {d}"),
        InstKind::ConstBigInt(n) => format!("bigint {n}"),
        InstKind::ConstNull => "null".to_owned(),
        InstKind::Binary {
            op,
            lhs,
            rhs,
            ty: t,
        } => {
            format!("{}.{} {}, {}", binop(*op), ty(*t), val(*lhs), val(*rhs))
        }
        InstKind::Unary { op, operand, ty: t } => {
            format!("{}.{} {}", unop(*op), ty(*t), val(*operand))
        }
        InstKind::LoadGlobal(name) => format!("global {name}"),
        InstKind::LoadUpvalue(uv) => format!("upvalue #{uv}"),
        InstKind::StoreGlobal { name, value } => format!("storeglobal {name} = {}", val(*value)),
        InstKind::StoreUpvalue { index, value } => {
            format!("storeupvalue #{index} = {}", val(*value))
        }
        InstKind::Call { callee, args } => format!("call {}{}", val(*callee), args_list(args)),
        InstKind::SelfCall { args } => format!("callself{}", args_list(args)),
        InstKind::GetProperty { object, name } => format!("getprop {}.{name}", val(*object)),
        InstKind::GetFixedField { object, slot } => format!("getfixed {}[{slot}]", val(*object)),
        InstKind::GetIndex { object, index } => {
            format!("getindex {}[{}]", val(*object), val(*index))
        }
        InstKind::ArrayGetIndex { object, index } => {
            format!("arraygetindex {}[{}]", val(*object), val(*index))
        }
        InstKind::SetProperty {
            object,
            name,
            value,
        } => {
            format!("setprop {}.{name} = {}", val(*object), val(*value))
        }
        InstKind::SetFixedField {
            object,
            value,
            slot,
        } => {
            format!("setfixed {}[{slot}] = {}", val(*object), val(*value))
        }
        InstKind::SetIndex {
            object,
            index,
            value,
        } => {
            format!(
                "setindex {}[{}] = {}",
                val(*object),
                val(*index),
                val(*value)
            )
        }
        InstKind::ArrayPush { array, value } => {
            format!("arraypush {}, {}", val(*array), val(*value))
        }
        InstKind::ArraySetIndex {
            object,
            index,
            value,
        } => {
            format!(
                "arraysetindex {}[{}] = {}",
                val(*object),
                val(*index),
                val(*value)
            )
        }
        InstKind::ObjectMerge { target, source } => {
            format!("objectmerge {} <- {}", val(*target), val(*source))
        }
        InstKind::MethodCall { recv, name, args } => {
            format!("callmethod {}.{name}{}", val(*recv), args_list(args))
        }
        InstKind::IsNull { operand } => format!("isnull {}", val(*operand)),
        InstKind::Cast { operand, ty } => format!("cast {} as {ty:?}", val(*operand)),
        InstKind::BuildArray { elements } => format!("array{}", args_list(elements)),
        InstKind::BuildTuple { elements } => format!("tuple{}", args_list(elements)),
        InstKind::BuildObject { pairs } => {
            let inner = pairs
                .iter()
                .map(|(k, v)| format!("{k}: {}", val(*v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("object {{{inner}}}")
        }
        InstKind::BuildRecord { pairs } => {
            let inner = pairs
                .iter()
                .map(|(k, v)| format!("{k}: {}", val(*v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("record {{{inner}}}")
        }
        InstKind::ObjectRest { object, skip_keys } => {
            format!("objectrest {} skip={:?}", val(*object), skip_keys)
        }
        InstKind::ToString { operand } => format!("tostring {}", val(*operand)),
        InstKind::BuildStr { parts } => format!("buildstr{}", args_list(parts)),
        InstKind::MakeClosure { func, .. } => format!("closure {}", func.name),
        InstKind::IntrinsicCall {
            object,
            args,
            wire_byte,
        } => {
            format!("intrinsic#{wire_byte} {}{}", val(*object), args_list(args))
        }
        InstKind::CallNativeOp {
            object,
            args,
            op_id,
        } => {
            format!("nativeop#{op_id:#x} {}{}", val(*object), args_list(args))
        }
        InstKind::AssertNotNull { operand } => format!("assertnotnull {}", val(*operand)),
        InstKind::GetPropertyMaybe { object, name } => {
            format!("getpropmaybe {}.{name}", val(*object))
        }
        InstKind::ModuleSlot { object, slot } => format!("moduleslot {}[{slot}]", val(*object)),
        InstKind::GetEnumTag { operand } => format!("enumtag {}", val(*operand)),
        InstKind::IsArray { operand } => format!("isarray {}", val(*operand)),
        InstKind::This => "this".to_owned(),
        InstKind::Range {
            start,
            end,
            inclusive,
        } => {
            let op = if *inclusive { "..=" } else { ".." };
            format!("range {}{op}{}", val(*start), val(*end))
        }
        InstKind::ObjectKeys { operand } => format!("objectkeys {}", val(*operand)),
        InstKind::GetSymbol { object, is_async } => {
            let s = if *is_async {
                "asyncIterator"
            } else {
                "iterator"
            };
            format!("getsymbol {}.@@{s}", val(*object))
        }
        InstKind::IterCall { callee, recv } => {
            format!("itercall {}({})", val(*callee), val(*recv))
        }
        InstKind::GetSuper { name } => format!("getsuper super.{name}"),
        InstKind::SuperCall { args } => format!("supercall{}", args_list(args)),
        InstKind::SuperMethodCall { name, args } => {
            format!("supercall super.{name}{}", args_list(args))
        }
        InstKind::ExtensionCall { func, recv, args } => {
            format!("extcall {func}({}{})", val(*recv), {
                let a = args.iter().map(|v| val(*v)).collect::<Vec<_>>().join(", ");
                if a.is_empty() {
                    String::new()
                } else {
                    format!(", {a}")
                }
            })
        }
        InstKind::CallSpread { callee, args } => {
            let a = args
                .iter()
                .map(|(v, s)| {
                    if *s {
                        format!("...{}", val(*v))
                    } else {
                        val(*v)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("callspread {}({a})", val(*callee))
        }
        InstKind::BuildArraySpread { elements } => {
            let a = elements
                .iter()
                .map(|(v, s)| {
                    if *s {
                        format!("...{}", val(*v))
                    } else {
                        val(*v)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("array[{a}]")
        }
        InstKind::BuildObjectSpread { parts } => {
            let a = parts
                .iter()
                .map(|(k, v)| match k {
                    Some(k) => format!("{k}: {}", val(*v)),
                    None => format!("...{}", val(*v)),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("object {{{a}}}")
        }
        InstKind::LoadCaptured { var } => format!("loadcaptured {var:?}"),
        InstKind::StoreCaptured { var, value } => {
            format!("storecaptured {var:?} = {}", val(*value))
        }
        InstKind::MakeClass { name, super_class } => {
            format!("makeclass {name} super={:?}", super_class.map(val))
        }
        InstKind::DeclareField { class, name } => format!("declarefield {}.{name}", val(*class)),
        InstKind::DefineStatic { class, name, value } => {
            format!("definestatic {}.{name} = {}", val(*class), val(*value))
        }
        InstKind::DefineMethod {
            class,
            name,
            method,
            is_static,
        } => format!(
            "definemethod {}.{name} = {} (static={is_static})",
            val(*class),
            val(*method)
        ),
        InstKind::DefineAccessor {
            class,
            name,
            accessor,
            is_getter,
            is_static,
        } => format!(
            "defineaccessor {}.{name} = {} (getter={is_getter}, static={is_static})",
            val(*class),
            val(*accessor)
        ),
        InstKind::MakeEnumVariant { tag, meta } => format!("makeenumvariant tag={tag} meta={meta}"),
        InstKind::Try { handler } => format!("try b{}", handler.0),
        InstKind::PopTry => "poptry".to_owned(),
        InstKind::CatchParam { try_val } => format!("catchparam {}", val(*try_val)),
        InstKind::CloseUpvalues { targets } => format!("closeupvalues {:?}", targets),
        InstKind::Dispose { target, is_await } => format!("dispose {target:?} await={is_await}"),
        InstKind::LoadModule { source } => format!("loadmodule {source}"),
        InstKind::StoreModuleSlot { value, slot } => {
            format!("storemoduleslot {slot} = {}", val(*value))
        }
        InstKind::Await { operand } => format!("await {}", val(*operand)),
        InstKind::Spawn { operand } => format!("spawn {}", val(*operand)),
        InstKind::Yield { operand } => format!("yield {}", val(*operand)),
    }
}

fn terminator(term: &Terminator) -> String {
    match term {
        Terminator::Return(Some(v)) => format!("return {}", val(*v)),
        Terminator::Return(None) => "return".to_owned(),
        Terminator::Throw(v) => format!("throw {}", val(*v)),
        Terminator::Jump { target, args } => format!("jump b{}{}", target.0, args_list(args)),
        Terminator::Branch {
            cond,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } => format!(
            "branch {}, b{}{}, b{}{}",
            val(*cond),
            then_blk.0,
            args_list(then_args),
            else_blk.0,
            args_list(else_args),
        ),
        Terminator::Unreachable => "unreachable".to_owned(),
    }
}

fn args_list(args: &[Value]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        let inner = args.iter().map(|v| val(*v)).collect::<Vec<_>>().join(", ");
        format!("({inner})")
    }
}

fn val(v: Value) -> String {
    format!("v{}", v.0)
}

fn ty(t: HirType) -> &'static str {
    use varn_core::IntrinsicType;
    match t {
        HirType::Int => IntrinsicType::Int.as_str(),
        HirType::Float => IntrinsicType::Float.as_str(),
        HirType::Bool => IntrinsicType::Bool.as_str(),
        HirType::Str => IntrinsicType::Str.as_str(),
        HirType::Ref => "ref",
        HirType::Dynamic => "dyn",
        // Nested TyIds need the module's TyTable to render; the dump shows
        // the shape only.
        HirType::Array(_) => "array",
        HirType::Map(_, _) => "map",
        HirType::Set(_) => "set",
        HirType::Class(_) => "class",
        HirType::Nullable(_) => "nullable",
    }
}

fn binop(op: HirBinOp) -> &'static str {
    match op {
        HirBinOp::Add => "add",
        HirBinOp::Sub => "sub",
        HirBinOp::Mul => "mul",
        HirBinOp::Div => "div",
        HirBinOp::Mod => "mod",
        HirBinOp::Pow => "pow",
        HirBinOp::Eq => "eq",
        HirBinOp::Ne => "ne",
        HirBinOp::Lt => "lt",
        HirBinOp::Le => "le",
        HirBinOp::Gt => "gt",
        HirBinOp::Ge => "ge",
        HirBinOp::And => "and",
        HirBinOp::Or => "or",
        HirBinOp::BitAnd => "band",
        HirBinOp::BitOr => "bor",
        HirBinOp::BitXor => "bxor",
        HirBinOp::Shl => "shl",
        HirBinOp::Shr => "shr",
        HirBinOp::Ushr => "ushr",
        HirBinOp::Instanceof => "instanceof",
        HirBinOp::In => "in",
    }
}

fn unop(op: HirUnOp) -> &'static str {
    match op {
        HirUnOp::Neg => "neg",
        HirUnOp::Not => "not",
        HirUnOp::BitNot => "bnot",
        HirUnOp::Typeof => "typeof",
    }
}
