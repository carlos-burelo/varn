







mod ast_json;
mod cfg;

use varn_checker::module_resolver::ImportResolver;
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
        "varn.memoryStats" => Ok(Some(memory_stats(workspace))),
        _ => Err(format!("Unknown command: {command}")),
    }
}




fn memory_stats(workspace: &Workspace) -> serde_json::Value {
    let (graph_binds, graph_programs, graph_arenas, graph_exports) =
        crate::workspace::resolver::with_resolver(|r| r.graph_stats());

    
    
    
    
    
    let mut source_bytes: u64 = 0;
    let mut token_count: u64 = 0;
    let mut token_lexeme_bytes: u64 = 0;
    let mut symbol_count: u64 = 0;
    let mut scope_count: u64 = 0;
    let mut per_doc_interner_entries: u64 = 0;
    let mut expr_node_count: u64 = 0;
    let mut expr_node_bytes: u64 = 0;
    let mut expr_table_entries: u64 = 0;
    let mut expr_types_entries: u64 = 0;
    let mut node_scopes_entries: u64 = 0;
    let mut scope_spans_entries: u64 = 0;
    let mut symbol_types_entries: u64 = 0;
    let mut member_resolutions_entries: u64 = 0;
    let mut call_resolutions_entries: u64 = 0;
    let mut match_gaps_entries: u64 = 0;
    let mut call_mappings_entries: u64 = 0;
    let mut flattened_members_entries: u64 = 0;

    for entry in workspace.iter() {
        let state = entry.value();
        source_bytes += state.source.len() as u64;
        token_count += state.tokens.len() as u64;
        token_lexeme_bytes += state
            .tokens
            .iter()
            .map(|t| (t.end - t.offset) as u64)
            .sum::<u64>();
        symbol_count += state.db.bind.arena.all().len() as u64;
        scope_count += state.db.bind.scopes.len() as u64;
        per_doc_interner_entries += state.db.bind.interner.len() as u64;
        let n = state.ast_arena.exprs().count() as u64;
        expr_node_count += n;
        expr_node_bytes += n * std::mem::size_of::<varn_core::ast::arena::ExprNode>() as u64;
        expr_table_entries += state.db.expr_table.len() as u64;
        expr_types_entries += state.db.expr_types.len() as u64;
        node_scopes_entries += state.db.node_scopes.len() as u64;
        scope_spans_entries += state.db.scope_spans.len() as u64;
        symbol_types_entries += state.db.symbol_types.len() as u64;
        member_resolutions_entries += state.db.member_resolutions.len() as u64;
        call_resolutions_entries += state.db.call_resolutions.len() as u64;
        match_gaps_entries += state.db.match_gaps.len() as u64;
        call_mappings_entries += state.db.call_mappings.len() as u64;
        flattened_members_entries += state.db.flattened_members.len() as u64;
    }

    serde_json::json!({
        "residentKb": crate::backend::mem::resident_kb(),
        "openDocuments": workspace.file_count(),
        "graphBinds": graph_binds,
        "graphPrograms": graph_programs,
        "graphArenas": graph_arenas,
        "graphExports": graph_exports,
        "perDocument": {
            "totalSourceBytes": source_bytes,
            "totalTokens": token_count,
            "totalTokenLexemeBytes": token_lexeme_bytes,
            "totalSymbolCount": symbol_count,
            "totalScopeCount": scope_count,
            "sumOfEachDocsInternerLen": per_doc_interner_entries,
            "totalExprNodeCount": expr_node_count,
            "totalExprNodeBytes": expr_node_bytes,
            "totalExprTableEntries": expr_table_entries,
            "totalExprTypesEntries": expr_types_entries,
            "totalNodeScopesEntries": node_scopes_entries,
            "totalScopeSpansEntries": scope_spans_entries,
            "totalSymbolTypesEntries": symbol_types_entries,
            "totalMemberResolutionsEntries": member_resolutions_entries,
            "totalCallResolutionsEntries": call_resolutions_entries,
            "totalMatchGapsEntries": match_gaps_entries,
            "totalCallMappingsEntries": call_mappings_entries,
            "totalFlattenedMembersEntries": flattened_members_entries,
        }
    })
}


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
