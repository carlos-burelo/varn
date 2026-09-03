use rustc_hash::FxHashMap;
use varn_compiler::FunctionProto;
use varn_types::chunk::PoolEntry;

use crate::document::DocumentState;
use crate::workspace::Workspace;

pub fn execute_command(
    command: &str,
    arguments: Vec<serde_json::Value>,
    workspace: &Workspace,
) -> Result<Option<serde_json::Value>, String> {
    match command {
        "varn.showAst" | "varn.syntaxTree" => {
            let uri = arguments
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing URI argument".to_string())?;
            let state = workspace
                .get(uri)
                .ok_or_else(|| format!("Document not found: {uri}"))?;

            let ast_json = dump_ast_json(&state)?;
            Ok(Some(ast_json))
        }
        "varn.showBytecode" => {
            let uri = arguments
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing URI argument".to_string())?;
            let state = workspace
                .get(uri)
                .ok_or_else(|| format!("Document not found: {uri}"))?;

            let bytecode_text = compile_and_disassemble(&state)?;
            Ok(Some(serde_json::Value::String(bytecode_text)))
        }
        "varn.showSSA" => {
            let uri = arguments
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing URI argument".to_string())?;
            let state = workspace
                .get(uri)
                .ok_or_else(|| format!("Document not found: {uri}"))?;

            let ssa_text = compile_and_dump_ssa(&state)?;
            Ok(Some(serde_json::Value::String(ssa_text)))
        }
        "varn.getCFG" => {
            let uri = arguments
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing URI argument".to_string())?;
            let state = workspace
                .get(uri)
                .ok_or_else(|| format!("Document not found: {uri}"))?;

            let cfg_json = compile_and_get_cfg_json(&state)?;
            Ok(Some(cfg_json))
        }
        "varn.evalSelection" => {
            let code = arguments
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing code argument".to_string())?;

            let result = format!("Evaluated: {code}");
            Ok(Some(serde_json::Value::String(result)))
        }
        _ => Err(format!("Unknown command: {command}")),
    }
}

pub fn compile_and_get_cfg_json(state: &DocumentState) -> Result<serde_json::Value, String> {
    let program = state
        .ast
        .as_ref()
        .ok_or_else(|| "No AST available".to_string())?;

    let dummy_annotations = varn_core::TypeAnnotations::default();
    let empty_map = FxHashMap::default();
    let export_names = Vec::new();

    let input = varn_compiler::OptInput {
        program,
        annotations: &dummy_annotations,
        extension_calls: &empty_map,
        extension_members: &empty_map,
        extension_set_members: &empty_map,
        export_names,
    };

    let ssa_res = varn_compiler::lower_to_ssa(input);

    let mut json_functions = Vec::new();

    if let Ok((funcs, _skipped)) = ssa_res {
        for func in funcs {
            let mut json_blocks = Vec::new();
            for (b_idx, block) in func.blocks.iter().enumerate() {
                let mut json_insts = Vec::new();
                for inst in &block.insts {
                    let dest_str = inst.dest.map(|d| format!("v{}", d.0));
                    let repr_str = format_inst_human(&inst.kind);
                    let op_name = format!("{:?}", inst.kind);
                    let op_short = op_name
                        .split(['(', ' ', '{'])
                        .next()
                        .unwrap_or(&op_name);

                    json_insts.push(serde_json::json!({
                        "dest": dest_str,
                        "op": op_short,
                        "repr": repr_str,
                        "line": inst.line,
                    }));
                }

                let mut successors = Vec::new();
                let term_repr = match &block.term {
                    varn_compiler::ssa::ir::Terminator::Return(v) => match v {
                        Some(val) => format!("return v{}", val.0),
                        None => "return".to_string(),
                    },
                    varn_compiler::ssa::ir::Terminator::Throw(v) => {
                        format!("throw v{}", v.0)
                    }
                    varn_compiler::ssa::ir::Terminator::Jump { target, args } => {
                        successors.push(serde_json::json!({
                            "target": format!("b{}", target.0),
                            "kind": "jump",
                            "args": args.iter().map(|a| format!("v{}", a.0)).collect::<Vec<_>>(),
                        }));
                        let a_str = args
                            .iter()
                            .map(|a| format!("v{}", a.0))
                            .collect::<Vec<_>>()
                            .join(", ");
                        if a_str.is_empty() {
                            format!("jump b{}", target.0)
                        } else {
                            format!("jump b{}({})", target.0, a_str)
                        }
                    }
                    varn_compiler::ssa::ir::Terminator::Branch {
                        cond,
                        then_blk,
                        else_blk,
                        ..
                    } => {
                        successors.push(serde_json::json!({
                            "target": format!("b{}", then_blk.0),
                            "kind": "true",
                            "cond": format!("v{}", cond.0),
                        }));
                        successors.push(serde_json::json!({
                            "target": format!("b{}", else_blk.0),
                            "kind": "false",
                            "cond": format!("v{}", cond.0),
                        }));
                        format!("branch v{} ? b{} : b{}", cond.0, then_blk.0, else_blk.0)
                    }
                    varn_compiler::ssa::ir::Terminator::Unreachable => "unreachable".to_string(),
                };

                let preds: Vec<String> =
                    block.preds.iter().map(|p| format!("b{}", p.0)).collect();
                let params: Vec<String> =
                    block.params.iter().map(|p| format!("v{}", p.0)).collect();

                json_blocks.push(serde_json::json!({
                    "id": format!("b{}", b_idx),
                    "params": params,
                    "preds": preds,
                    "insts": json_insts,
                    "terminator": term_repr,
                    "term_line": block.term_line,
                    "successors": successors,
                }));
            }

            json_functions.push(serde_json::json!({
                "name": func.name.as_ref(),
                "entry": format!("b{}", func.entry.0),
                "is_async": func.is_async,
                "is_generator": func.is_generator,
                "blocks": json_blocks,
            }));
        }
    }

    let bytecode_text = compile_and_disassemble(state).unwrap_or_default();

    Ok(serde_json::json!({
        "filename": program.filename,
        "functions": json_functions,
        "bytecode": bytecode_text,
    }))
}

fn format_inst_human(kind: &varn_compiler::ssa::ir::InstKind) -> String {
    use varn_compiler::ssa::ir::InstKind::*;
    match kind {
        ConstInt(n) => format!("{n}"),
        ConstFloat(f) => format!("{f}"),
        ConstBool(b) => format!("{b}"),
        ConstStr(s) => format!("\"{s}\""),
        ConstChar(c) => format!("'{c}'"),
        ConstDecimal(d) => format!("{d}m"),
        ConstBigInt(b) => format!("{b}n"),
        ConstNull => "null".to_string(),
        Binary { op, lhs, rhs, .. } => {
            let op_sym = match op {
                varn_compiler::hir::HirBinOp::Add => "+",
                varn_compiler::hir::HirBinOp::Sub => "-",
                varn_compiler::hir::HirBinOp::Mul => "*",
                varn_compiler::hir::HirBinOp::Div => "/",
                varn_compiler::hir::HirBinOp::Mod => "%",
                varn_compiler::hir::HirBinOp::Pow => "**",
                varn_compiler::hir::HirBinOp::Eq => "==",
                varn_compiler::hir::HirBinOp::Ne => "!=",
                varn_compiler::hir::HirBinOp::Lt => "<",
                varn_compiler::hir::HirBinOp::Le => "<=",
                varn_compiler::hir::HirBinOp::Gt => ">",
                varn_compiler::hir::HirBinOp::Ge => ">=",
                varn_compiler::hir::HirBinOp::And => "&&",
                varn_compiler::hir::HirBinOp::Or => "||",
                varn_compiler::hir::HirBinOp::BitAnd => "&",
                varn_compiler::hir::HirBinOp::BitOr => "|",
                varn_compiler::hir::HirBinOp::BitXor => "^",
                varn_compiler::hir::HirBinOp::Shl => "<<",
                varn_compiler::hir::HirBinOp::Shr => ">>",
                varn_compiler::hir::HirBinOp::Ushr => ">>>",
                varn_compiler::hir::HirBinOp::Instanceof => "instanceof",
                varn_compiler::hir::HirBinOp::In => "in",
            };
            format!("v{} {op_sym} v{}", lhs.0, rhs.0)
        }
        Unary { op, operand, .. } => {
            let op_sym = match op {
                varn_compiler::hir::HirUnOp::Neg => "-",
                varn_compiler::hir::HirUnOp::Not => "!",
                varn_compiler::hir::HirUnOp::BitNot => "~",
                varn_compiler::hir::HirUnOp::Typeof => "typeof ",
            };
            format!("{op_sym}v{}", operand.0)
        }
        LoadGlobal(name) => format!("LoadGlobal \"{name}\""),
        StoreGlobal { name, value } => format!("StoreGlobal \"{name}\" = v{}", value.0),
        LoadUpvalue(idx) => format!("LoadUpvalue [{idx}]"),
        StoreUpvalue { index, value } => format!("StoreUpvalue [{index}] = v{}", value.0),
        Call { callee, args } => {
            let args_str = args
                .iter()
                .map(|a| format!("v{}", a.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("v{}({})", callee.0, args_str)
        }
        SelfCall { args } => {
            let args_str = args
                .iter()
                .map(|a| format!("v{}", a.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("self_call({})", args_str)
        }
        GetProperty { object, name } => format!("v{}.{name}", object.0),
        SetProperty { object, name, value } => format!("v{}.{name} = v{}", object.0, value.0),
        GetFixedField { object, slot } => format!("v{}.slot[{slot}]", object.0),
        SetFixedField { object, slot, value } => {
            format!("v{}.slot[{slot}] = v{}", object.0, value.0)
        }
        GetIndex { object, index } => format!("v{}[v{}]", object.0, index.0),
        SetIndex { object, index, value } => {
            format!("v{}[v{}] = v{}", object.0, index.0, value.0)
        }
        ArrayGetIndex { object, index } => format!("v{}[v{}]", object.0, index.0),
        ArraySetIndex { object, index, value } => {
            format!("v{}[v{}] = v{}", object.0, index.0, value.0)
        }
        ArrayPush { array, value } => format!("push v{}, v{}", array.0, value.0),
        ObjectMerge { target, source } => format!("merge v{}, v{}", target.0, source.0),
        MethodCall { recv, name, args } => {
            let args_str = args
                .iter()
                .map(|a| format!("v{}", a.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("v{}.{}({})", recv.0, name, args_str)
        }
        IsNull { operand } => format!("v{} == null", operand.0),
        BuildArray { elements } => {
            let elems = elements
                .iter()
                .map(|e| format!("v{}", e.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{elems}]")
        }
        BuildTuple { elements } => {
            let elems = elements
                .iter()
                .map(|e| format!("v{}", e.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({elems})")
        }
        BuildObject { pairs } => {
            let p_str = pairs
                .iter()
                .map(|(k, v)| format!("{k}: v{}", v.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{p_str}}}")
        }
        BuildRecord { pairs } => {
            let p_str = pairs
                .iter()
                .map(|(k, v)| format!("{k}: v{}", v.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("record {{{p_str}}}")
        }
        ToString { operand } => format!("toString(v{})", operand.0),
        BuildStr { parts } => {
            let p_str = parts
                .iter()
                .map(|p| format!("v{}", p.0))
                .collect::<Vec<_>>()
                .join(" + ");
            format!("build_str({p_str})")
        }
        MakeClosure { func, .. } => format!("make_closure @{}", func.name),
        LoadCaptured { var } => format!("load_captured {var:?}"),
        StoreCaptured { var, value } => format!("store_captured {var:?} = v{}", value.0),
        MakeClass { name, .. } => format!("make_class \"{name}\""),
        DeclareField { class, name } => format!("declare_field v{}.{name}", class.0),
        DefineStatic { class, name, value } => {
            format!("define_static v{}.{name} = v{}", class.0, value.0)
        }
        DefineMethod {
            class,
            name,
            method,
            ..
        } => format!("define_method v{}.{name} = v{}", class.0, method.0),
        DefineAccessor {
            class,
            name,
            accessor,
            is_getter,
            ..
        } => {
            let kind = if *is_getter { "get" } else { "set" };
            format!(
                "define_accessor {kind} v{}.{name} = v{}",
                class.0, accessor.0
            )
        }
        MakeEnumVariant { tag, meta } => format!("enum_variant #{tag} ({meta})"),
        Try { handler } => format!("try -> b{}", handler.0),
        PopTry => "pop_try".to_string(),
        CatchParam { try_val } => format!("catch_param v{}", try_val.0),
        CloseUpvalues { targets } => format!("close_upvalues ({targets:?})"),
        Dispose { target, is_await } => format!("dispose ({target:?}, await: {is_await})"),
        LoadModule { source } => format!("import \"{source}\""),
        StoreModuleSlot { value, slot } => format!("module_slot[{slot}] = v{}", value.0),
        Await { operand } => format!("await v{}", operand.0),
        Spawn { operand } => format!("spawn v{}", operand.0),
        Yield { operand } => format!("yield v{}", operand.0),
        IntrinsicCall {
            object,
            args,
            wire_byte,
        } => {
            let args_str = args
                .iter()
                .map(|a| format!("v{}", a.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("intrinsic#{wire_byte}(v{}, [{}])", object.0, args_str)
        }
        CallNativeOp {
            object,
            args,
            op_id,
        } => {
            let args_str = args
                .iter()
                .map(|a| format!("v{}", a.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("native_op#{op_id}(v{}, [{}])", object.0, args_str)
        }
        AssertNotNull { operand } => format!("assert_not_null(v{})", operand.0),
        GetPropertyMaybe { object, name } => format!("v{}?.{name}", object.0),
        ModuleSlot { object, slot } => format!("v{}.slot[{slot}]", object.0),
        GetEnumTag { operand } => format!("enum_tag(v{})", operand.0),
        IsArray { operand } => format!("is_array(v{})", operand.0),
        This => "this".to_string(),
        Range {
            start,
            end,
            inclusive,
        } => {
            let op = if *inclusive { "..=" } else { ".." };
            format!("v{} {op} v{}", start.0, end.0)
        }
        ObjectKeys { operand } => format!("Object.keys(v{})", operand.0),
        GetSymbol { object, is_async } => {
            format!("get_symbol(v{}, async: {is_async})", object.0)
        }
        IterCall { callee, recv } => {
            format!("iter_call(callee: v{}, recv: v{})", callee.0, recv.0)
        }
        GetSuper { name } => format!("super.{name}"),
        SuperCall { args } => {
            let args_str = args
                .iter()
                .map(|a| format!("v{}", a.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("super({args_str})")
        }
        SuperMethodCall { name, args } => {
            let args_str = args
                .iter()
                .map(|a| format!("v{}", a.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("super.{name}({args_str})")
        }
        ExtensionCall { func, recv, args } => {
            let args_str = args
                .iter()
                .map(|a| format!("v{}", a.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{func}(v{}, {})", recv.0, args_str)
        }
        CallSpread { callee, .. } => format!("call_spread v{}", callee.0),
        BuildArraySpread { .. } => "build_array_spread [...]".to_string(),
        BuildObjectSpread { .. } => "build_object_spread {...}".to_string(),
        ObjectRest { .. } => "object_rest".to_string(),
        Cast { operand, ty } => format!("cast v{} as {:?}", operand.0, ty),
    }
}

pub fn compile_and_disassemble(state: &DocumentState) -> Result<String, String> {
    let program = state
        .ast
        .as_ref()
        .ok_or_else(|| "No AST available".to_string())?;

    let dummy_annotations = varn_core::TypeAnnotations::default();
    let empty_map = FxHashMap::default();
    let export_names = Vec::new();

    let proto = varn_compiler::compile_module(
        program,
        &dummy_annotations,
        &empty_map,
        &empty_map,
        &empty_map,
        export_names,
    )
    .map_err(|e| format!("Compilation failed: {e}"))?;

    let mut out = String::new();
    format_proto(&proto, 0, &mut out);
    Ok(out)
}

pub fn compile_and_dump_ssa(state: &DocumentState) -> Result<String, String> {
    let program = state
        .ast
        .as_ref()
        .ok_or_else(|| "No AST available".to_string())?;

    let dummy_annotations = varn_core::TypeAnnotations::default();
    let empty_map = FxHashMap::default();
    let export_names = Vec::new();

    let input = varn_compiler::OptInput {
        program,
        annotations: &dummy_annotations,
        extension_calls: &empty_map,
        extension_members: &empty_map,
        extension_set_members: &empty_map,
        export_names,
    };

    let mut module = varn_compiler::hir::lower::lower_program(&input)
        .map_err(|e| format!("HIR lowering failed: {e:?}"))?;
    varn_compiler::hir::inline::run(&mut module);
    varn_compiler::hir::module_locals::run(&mut module);

    let mut out = String::new();
    out.push_str(&format!("; Varn HIR/SSA Module: {}\n", program.filename));
    out.push_str(&format!("; Top-level + {} functions\n\n", module.functions.len()));

    out.push_str(&format!("fn @top_level (params: {}, locals: {})\n", module.top_level.params.len(), module.top_level.locals));
    for (idx, func) in module.functions.iter().enumerate() {
        out.push_str(&format!("fn #{idx} @{} (params: {}, locals: {})\n", func.name, func.params.len(), func.locals));
    }

    Ok(out)
}

fn format_proto(proto: &FunctionProto, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    let name = proto.name.as_deref().unwrap_or("<top-level>");
    out.push_str(&format!(
        "{}=== Function '{}' (arity: {}, registers: {}, upvalues: {}) ===\n",
        indent, name, proto.arity, proto.register_count, proto.upvalue_count
    ));

    out.push_str(&format!("{}Constants ({}):\n", indent, proto.chunk.constants.len()));
    for (idx, c) in proto.chunk.constants.iter().enumerate() {
        match c {
            PoolEntry::Literal(lit) => out.push_str(&format!("{}  [{:03}] Literal: {:?}\n", indent, idx, lit)),
            PoolEntry::Function(f) => {
                let fname = f.name.as_deref().unwrap_or("<anonymous>");
                out.push_str(&format!("{}  [{:03}] Function: {}\n", indent, idx, fname));
            }
            PoolEntry::Shape(keys) => out.push_str(&format!("{}  [{:03}] Shape: [{}]\n", indent, idx, keys.join(", "))),
        }
    }

    out.push_str(&format!("{}Bytecode ({} instructions):\n", indent, proto.chunk.code.len()));
    let mut ip = 0;
    while ip < proto.chunk.code.len() {
        let op_u16 = proto.chunk.code[ip];
        let op_byte = (op_u16 & 0xFF) as u8;
        let op = varn_core::OpCode::from_u8(op_byte);
        let reg_a = (op_u16 >> 8) as u8;

        out.push_str(&format!("{}  {:04} | r{} {:?}\n", indent, ip, reg_a, op));
        ip += 1;
    }
    out.push('\n');

    // Recursively format nested functions
    for c in &proto.chunk.constants {
        if let PoolEntry::Function(sub_proto) = c {
            format_proto(sub_proto, depth + 1, out);
        }
    }
}

pub fn dump_ast_json(state: &DocumentState) -> Result<serde_json::Value, String> {
    let program = state
        .ast
        .as_ref()
        .ok_or_else(|| "No AST available".to_string())?;

    let mut roots = Vec::new();
    for stmt in &program.body {
        roots.push(ast_stmt_to_json(stmt));
    }

    Ok(serde_json::Value::Array(roots))
}

fn make_ast_node(label: String, kind: &str, line: u32, children: Vec<serde_json::Value>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("label".to_string(), serde_json::Value::String(label));
    map.insert("kind".to_string(), serde_json::Value::String(kind.to_string()));
    map.insert("line".to_string(), serde_json::Value::Number(serde_json::Number::from(line.saturating_sub(1))));
    if !children.is_empty() {
        map.insert("children".to_string(), serde_json::Value::Array(children));
    }
    serde_json::Value::Object(map)
}

fn ast_stmt_to_json(stmt: &varn_core::ast::Stmt) -> serde_json::Value {
    use varn_core::ast::{Decl, StmtKind};
    let line = stmt.range.start.line;
    match &stmt.kind {
        StmtKind::Decl(decl) => match decl.as_ref() {
            Decl::Function(f) => {
                let mut children = Vec::new();
                for param in &f.params {
                    children.push(make_ast_node(
                        format!("param: {:?}", param.pattern),
                        "parameter",
                        param.range.start.line,
                        vec![],
                    ));
                }
                children.push(ast_stmt_to_json(&f.body));
                make_ast_node(format!("fn {}", f.id), "function", line, children)
            }
            Decl::Class(c) => {
                let class_name = c.id.as_deref().unwrap_or("Anonymous");
                let mut children = Vec::new();
                for member in &c.body {
                    match member {
                        varn_core::ast::ClassMember::Property { key, range, .. } => {
                            children.push(make_ast_node(
                                format!("prop {key}"),
                                "property",
                                range.start.line,
                                vec![],
                            ));
                        }
                        varn_core::ast::ClassMember::Method { key, body, range, .. } => {
                            let mut method_children = Vec::new();
                            if let Some(b) = body {
                                method_children.push(ast_stmt_to_json(b));
                            }
                            children.push(make_ast_node(
                                format!("method {key}"),
                                "method",
                                range.start.line,
                                method_children,
                            ));
                        }
                        varn_core::ast::ClassMember::Constructor { range, body, .. } => {
                            children.push(make_ast_node(
                                "constructor".to_string(),
                                "method",
                                range.start.line,
                                vec![ast_stmt_to_json(body)],
                            ));
                        }
                        _ => {}
                    }
                }
                make_ast_node(format!("class {class_name}"), "class", line, children)
            }
            Decl::Interface(i) => {
                let mut children = Vec::new();
                for m in &i.body {
                    match m {
                        varn_core::ast::InterfaceMember::Method { key, range, .. } => {
                            children.push(make_ast_node(
                                format!("method {key}"),
                                "method",
                                range.start.line,
                                vec![],
                            ));
                        }
                        varn_core::ast::InterfaceMember::Property { key, range, .. } => {
                            children.push(make_ast_node(
                                format!("prop {key}"),
                                "property",
                                range.start.line,
                                vec![],
                            ));
                        }
                        _ => {}
                    }
                }
                make_ast_node(format!("interface {}", i.id), "interface", line, children)
            }
            Decl::Enum(e) => {
                let children: Vec<_> = e
                    .members
                    .iter()
                    .map(|m| make_ast_node(format!("variant {}", m.id), "enum_member", m.range.start.line, vec![]))
                    .collect();
                make_ast_node(format!("enum {}", e.id), "enum", line, children)
            }
            Decl::Struct(s) => {
                make_ast_node(format!("struct {}", s.id), "struct", line, vec![])
            }
            Decl::TypeAlias(t) => {
                make_ast_node(format!("type {}", t.id), "type", line, vec![])
            }
            Decl::Import(imp) => {
                make_ast_node(format!("import '{}'", imp.source), "import", line, vec![])
            }
            Decl::Export(exp) => match exp {
                varn_core::ast::ExportDecl::Decl { declaration, .. } => {
                    make_ast_node("export".to_string(), "export", line, vec![ast_stmt_to_json(&varn_core::ast::Stmt {
                        id: 0,
                        kind: varn_core::ast::StmtKind::Decl(declaration.clone()),
                        range: stmt.range,
                    })])
                }
                _ => make_ast_node("export".to_string(), "export", line, vec![]),
            },
            Decl::Variable(v) => {
                let children: Vec<_> = v
                    .declarators
                    .iter()
                    .map(|d| {
                        let val_children = d.init.as_ref().map(|init| vec![ast_expr_to_json(init)]).unwrap_or_default();
                        make_ast_node(format!("{:?} {:?}", v.kind, d.id), "variable", d.range.start.line, val_children)
                    })
                    .collect();
                make_ast_node(format!("{:?}Decl", v.kind), "variable", line, children)
            }
            Decl::Extension(ext) => {
                make_ast_node(format!("extension {:?}", ext.target), "class", line, vec![])
            }
            _ => make_ast_node("Decl".to_string(), "field", line, vec![]),
        },
        StmtKind::Block { stmts } => {
            let children = stmts.iter().map(ast_stmt_to_json).collect();
            make_ast_node("Block".to_string(), "method", line, children)
        }
        StmtKind::Expr { expression } => {
            make_ast_node("ExprStmt".to_string(), "statement", line, vec![ast_expr_to_json(expression)])
        }
        StmtKind::If { test, consequent, alternate } => {
            let mut children = vec![ast_expr_to_json(test), ast_stmt_to_json(consequent)];
            if let Some(alt) = alternate {
                children.push(ast_stmt_to_json(alt));
            }
            make_ast_node("IfStmt".to_string(), "keyword", line, children)
        }
        StmtKind::While { test, body } => {
            make_ast_node("WhileStmt".to_string(), "keyword", line, vec![ast_expr_to_json(test), ast_stmt_to_json(body)])
        }
        StmtKind::For { init, test, update, body } => {
            let mut children = Vec::new();
            if let Some(i) = init {
                match &**i {
                    varn_core::ast::ForInit::Var { kind, declarators } => {
                        for d in declarators {
                            let val_children = d.init.as_ref().map(|init| vec![ast_expr_to_json(init)]).unwrap_or_default();
                            children.push(make_ast_node(format!("{:?} {:?}", kind, d.id), "variable", d.range.start.line, val_children));
                        }
                    }
                    varn_core::ast::ForInit::Expr(e) => children.push(ast_expr_to_json(e)),
                }
            }
            if let Some(t) = test { children.push(ast_expr_to_json(t)); }
            if let Some(u) = update { children.push(ast_expr_to_json(u)); }
            children.push(ast_stmt_to_json(body));
            make_ast_node("ForStmt".to_string(), "keyword", line, children)
        }
        StmtKind::ForIn { left, right, body, .. } => {
            make_ast_node(format!("for {:?} in", left), "keyword", line, vec![ast_expr_to_json(right), ast_stmt_to_json(body)])
        }
        StmtKind::ForOf { left, right, body, .. } => {
            make_ast_node(format!("for {:?} of", left), "keyword", line, vec![ast_expr_to_json(right), ast_stmt_to_json(body)])
        }
        StmtKind::Return { argument } => {
            let children = argument.as_ref().map(|a| vec![ast_expr_to_json(a)]).unwrap_or_default();
            make_ast_node("ReturnStmt".to_string(), "keyword", line, children)
        }
        StmtKind::Break { .. } => make_ast_node("BreakStmt".to_string(), "keyword", line, vec![]),
        StmtKind::Continue { .. } => make_ast_node("ContinueStmt".to_string(), "keyword", line, vec![]),
        StmtKind::Empty => make_ast_node("EmptyStmt".to_string(), "field", line, vec![]),
        StmtKind::Error => make_ast_node("ErrorStmt".to_string(), "field", line, vec![]),
        _ => make_ast_node("Stmt".to_string(), "statement", line, vec![]),
    }
}

fn ast_expr_to_json(expr: &varn_core::ast::Expr) -> serde_json::Value {
    use varn_core::ast::ExprKind;
    let line = expr.range.start.line;
    match &expr.kind {
        ExprKind::Identifier { name } => {
            make_ast_node(format!("Ident: {name}"), "variable", line, vec![])
        }
        ExprKind::IntLiteral { value, .. } => {
            make_ast_node(format!("Int: {value}"), "constant", line, vec![])
        }
        ExprKind::FloatLiteral { value, .. } => {
            make_ast_node(format!("Float: {value}"), "constant", line, vec![])
        }
        ExprKind::StrLiteral { value } => {
            make_ast_node(format!("Str: \"{value}\""), "constant", line, vec![])
        }
        ExprKind::BoolLiteral { value } => {
            make_ast_node(format!("Bool: {value}"), "constant", line, vec![])
        }
        ExprKind::CharLiteral { value } => {
            make_ast_node(format!("Char: '{value}'"), "constant", line, vec![])
        }
        ExprKind::NullLiteral => {
            make_ast_node("null".to_string(), "constant", line, vec![])
        }
        ExprKind::Binary { left, op, right } => {
            make_ast_node(
                format!("BinaryExpr ({op:?})"),
                "operator",
                line,
                vec![ast_expr_to_json(left), ast_expr_to_json(right)],
            )
        }
        ExprKind::Unary { op, operand, .. } => {
            make_ast_node(
                format!("UnaryExpr ({op:?})"),
                "operator",
                line,
                vec![ast_expr_to_json(operand)],
            )
        }
        ExprKind::Call { callee, args, .. } => {
            let mut children = vec![ast_expr_to_json(callee)];
            for arg in args {
                match arg {
                    varn_core::ast::Arg::Positional(e) | varn_core::ast::Arg::Spread(e) => {
                        children.push(ast_expr_to_json(e));
                    }
                    varn_core::ast::Arg::Named { label, value } => {
                        children.push(make_ast_node(format!("{label}:"), "parameter", value.range.start.line, vec![ast_expr_to_json(value)]));
                    }
                }
            }
            make_ast_node("CallExpr".to_string(), "keyword", line, children)
        }
        ExprKind::Member { object, property, computed, .. } => {
            let label = if *computed { "IndexExpr" } else { "MemberExpr" };
            make_ast_node(
                label.to_string(),
                "property",
                line,
                vec![ast_expr_to_json(object), ast_expr_to_json(property)],
            )
        }
        ExprKind::Pipeline { left, right } => {
            make_ast_node(
                "Pipeline (|>)".to_string(),
                "operator",
                line,
                vec![ast_expr_to_json(left), ast_expr_to_json(right)],
            )
        }
        ExprKind::Assign { op, target, value } => {
            make_ast_node(
                format!("AssignExpr ({op:?})"),
                "operator",
                line,
                vec![ast_expr_to_json(target), ast_expr_to_json(value)],
            )
        }
        ExprKind::Match { subject, cases } => {
            let mut children = vec![ast_expr_to_json(subject)];
            for case in cases {
                let case_child = match &case.body {
                    varn_core::ast::MatchBody::Expr(e) => ast_expr_to_json(e),
                    varn_core::ast::MatchBody::Block(b) => ast_stmt_to_json(b),
                };
                children.push(make_ast_node("MatchCase".to_string(), "keyword", case.range.start.line, vec![case_child]));
            }
            make_ast_node("MatchExpr".to_string(), "keyword", line, children)
        }
        _ => make_ast_node("Expr".to_string(), "statement", line, vec![]),
    }
}

