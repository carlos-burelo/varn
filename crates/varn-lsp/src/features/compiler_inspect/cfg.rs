//! The document's SSA as a control-flow graph, for the editor's CFG view.

use varn_compiler::ssa::dump;
use varn_compiler::ssa::ir::Terminator;

use crate::document::DocumentState;

pub fn compile_and_get_cfg_json(state: &DocumentState) -> Result<serde_json::Value, String> {
    let ssa_res = super::build_ssa(state);

    let mut json_functions = Vec::new();

    if let Ok(funcs) = ssa_res {
        for func in funcs {
            let mut json_blocks = Vec::new();
            for (b_idx, block) in func.blocks.iter().enumerate() {
                let mut json_insts = Vec::new();
                for inst in &block.insts {
                    let dest_str = inst.dest.map(|d| format!("v{}", d.0));
                    let repr_str = dump::inst_kind(&inst.kind);
                    let op_name = format!("{:?}", inst.kind);
                    let op_short = op_name.split(['(', ' ', '{']).next().unwrap_or(&op_name);

                    json_insts.push(serde_json::json!({
                        "dest": dest_str,
                        "op": op_short,
                        "repr": repr_str,
                        "line": inst.line,
                    }));
                }

                let successors = successors_json(&block.term);
                let term_repr = dump::terminator(&block.term);

                let preds: Vec<String> = block.preds.iter().map(|p| format!("b{}", p.0)).collect();
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

    let bytecode_text = super::compile_and_disassemble(state).unwrap_or_default();
    let filename = state.ast.as_ref().map_or("", |p| p.filename.as_ref());

    Ok(serde_json::json!({
        "filename": filename,
        "functions": json_functions,
        "bytecode": bytecode_text,
    }))
}

/// The edges out of a block, for the graph view.
fn successors_json(term: &Terminator) -> Vec<serde_json::Value> {
    match term {
        Terminator::Jump { target, args } => vec![serde_json::json!({
            "target": format!("b{}", target.0),
            "kind": "jump",
            "args": args.iter().map(|a| format!("v{}", a.0)).collect::<Vec<_>>(),
        })],
        Terminator::Branch {
            cond,
            then_blk,
            else_blk,
            ..
        } => vec![
            serde_json::json!({
                "target": format!("b{}", then_blk.0),
                "kind": "true",
                "cond": format!("v{}", cond.0),
            }),
            serde_json::json!({
                "target": format!("b{}", else_blk.0),
                "kind": "false",
                "cond": format!("v{}", cond.0),
            }),
        ],
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
    }
}
