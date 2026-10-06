use super::super::ir::InstKind;
use super::operands::{args_list, binop, ty, unop, val};

pub fn inst_kind(kind: &InstKind) -> String {
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
        InstKind::LoadGlobalIdx(slot) => format!("global @{slot}"),
        InstKind::LoadNativeGlobalIdx(slot) => format!("native-global @{slot}"),
        InstKind::LoadUpvalue(uv) => format!("upvalue #{uv}"),
        InstKind::StoreGlobal { name, value } => format!("storeglobal {name} = {}", val(*value)),
        InstKind::StoreGlobalIdx { slot, value } => {
            format!("storeglobal @{slot} = {}", val(*value))
        }
        InstKind::StoreUpvalue { index, value } => {
            format!("storeupvalue #{index} = {}", val(*value))
        }
        InstKind::Call { callee, args } => format!("call {}{}", val(*callee), args_list(args)),
        InstKind::AllocInstance { class } => format!("alloc {}", val(*class)),
        InstKind::SelfCall { args } => format!("callself{}", args_list(args)),
        InstKind::GetProperty { object, name } => format!("getprop {}.{name}", val(*object)),
        InstKind::GetFixedField { object, slot, .. } => {
            format!("getfixed {}[{slot}]", val(*object))
        }
        InstKind::GetIndex { object, index } => {
            format!("getindex {}[{}]", val(*object), val(*index))
        }
        InstKind::ArrayGetIndex { object, index } => {
            format!("arraygetindex {}[{}]", val(*object), val(*index))
        }
        InstKind::MapGetIndex { object, index } => {
            format!("mapgetindex {}[{}]", val(*object), val(*index))
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
            ..
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
        InstKind::MapSetIndex {
            object,
            index,
            value,
        } => {
            format!(
                "mapsetindex {}[{}] = {}",
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
        InstKind::Convert { operand, conv } => format!("convert {} {conv:?}", val(*operand)),
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
        InstKind::BuildMap { pairs } => {
            let inner = pairs
                .iter()
                .map(|(k, v)| format!("{}: {}", val(*k), val(*v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("map {{{inner}}}")
        }
        InstKind::ObjectRest { object, skip_keys } => {
            format!("objectrest {} skip={:?}", val(*object), skip_keys)
        }
        InstKind::ToString { operand } => format!("tostring {}", val(*operand)),
        InstKind::BuildStr { parts } => format!("buildstr{}", args_list(parts)),
        InstKind::MakeClosure { func, .. } => format!("closure tir#{func}"),
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
        InstKind::StrLength { operand } => format!("strlen {}", val(*operand)),
        InstKind::ArrayLength { operand } => format!("arrlen {}", val(*operand)),
        InstKind::BytesLength { operand } => format!("byteslen {}", val(*operand)),
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
        InstKind::ExtensionCall {
            func, recv, args, ..
        } => {
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
        InstKind::DeclareLayout { class, layout } => {
            format!(
                "declarelayout {} {{{}}}",
                val(*class),
                layout
                    .fields
                    .iter()
                    .map(|f| format!("{}@{}", f.name, f.offset))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
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
