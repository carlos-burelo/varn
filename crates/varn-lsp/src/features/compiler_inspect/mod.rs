//! The compiler's views of a document, for the editor's inspection
//! commands: its syntax tree, its SSA (as text and as a control-flow graph)
//! and its bytecode.
//!
//! Each view lowers the document the way a compile does — the checker's
//! types, named-argument layouts and desugarings into TIR, then SSA and
//! bytecode — so what the editor shows is what would run.

mod ast_json;
mod cfg;

use varn_tir::TirModule;

use crate::document::DocumentState;
use crate::workspace::Workspace;

pub use ast_json::dump_ast_json;
pub use cfg::compile_and_get_cfg_json;

pub fn execute_command(
    command: &str,
    arguments: Vec<serde_json::Value>,
    workspace: &Workspace,
) -> Result<Option<serde_json::Value>, String> {
    let document = || -> Result<std::sync::Arc<DocumentState>, String> {
        let uri = arguments
            .first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing URI argument".to_string())?;
        workspace
            .get(uri)
            .ok_or_else(|| format!("Document not found: {uri}"))
    };
    match command {
        "varn.showAst" | "varn.syntaxTree" => Ok(Some(dump_ast_json(&*document()?)?)),
        "varn.showBytecode" => Ok(Some(serde_json::Value::String(compile_and_disassemble(
            &*document()?,
        )?))),
        "varn.showSSA" => Ok(Some(serde_json::Value::String(compile_and_dump_ssa(
            &*document()?,
        )?))),
        "varn.getCFG" => Ok(Some(compile_and_get_cfg_json(&*document()?)?)),
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

/// The document lowered to TIR, as a compile lowers it.
fn emit_tir(state: &DocumentState) -> Result<TirModule, String> {
    let program = state
        .ast
        .as_ref()
        .ok_or_else(|| "No AST available".to_string())?;
    Ok(varn_checker::emit::emit_module(
        program,
        &state.ast_arena,
        &state.db.bind,
        &state.db.expr_table,
        &state.db.call_mappings,
        &state.db.desugar,
    ))
}

/// The document's SSA functions: the top level first, then each function.
fn build_ssa(state: &DocumentState) -> Result<Vec<varn_compiler::ssa::ir::SsaFunc>, String> {
    varn_compiler::from_tir::build_module(&emit_tir(state)?)
        .map_err(|e| format!("SSA build failed: {e:?}"))
}

pub fn compile_and_disassemble(state: &DocumentState) -> Result<String, String> {
    let proto = varn_compiler::from_tir::compile_module(&emit_tir(state)?, Vec::new())
        .map_err(|e| format!("Compilation failed: {e:?}"))?;
    Ok(varn_types::bytecode::disasm::render(&proto))
}

pub fn compile_and_dump_ssa(state: &DocumentState) -> Result<String, String> {
    let fns = build_ssa(state)?;
    let filename = state.ast.as_ref().map_or("", |p| p.filename.as_ref());
    let mut out = format!(
        "; Varn TIR/SSA Module: {filename}\n; {} SSA function(s)\n\n",
        fns.len()
    );
    for func in &fns {
        out.push_str(&varn_compiler::ssa::dump::dump(func));
        out.push('\n');
    }
    Ok(out)
}
