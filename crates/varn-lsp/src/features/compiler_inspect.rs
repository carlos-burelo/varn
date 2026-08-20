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
