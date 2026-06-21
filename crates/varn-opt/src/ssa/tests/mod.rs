//! SSA construction tests: build from hand-written HIR, assert the dump.

use crate::hir::{HirBinOp, HirBinding, HirExpr, HirFunction, HirParam, HirStmt, HirType, LocalId};

use super::build::build_function;
use super::dump::dump;

fn func(params: Vec<HirType>, locals: u32, body: Vec<HirStmt>) -> HirFunction {
    HirFunction {
        name: "f".into(),
        params: params
            .into_iter()
            .map(|ty| HirParam {
                name: "p".into(),
                ty,
                default: None,
            })
            .collect(),
        locals,
        body,
        return_ty: HirType::Int,
        upvalue_count: 0,
        is_async: false,
        is_generator: false,
        has_this: false,
        has_rest: false,
    }
}

fn build(f: &HirFunction) -> String {
    dump(&build_function(f, &[]).expect("ssa build"))
}

/// Build a function to SSA and assert the verifier accepts it.
fn verify_ok(f: &HirFunction) {
    let ssa = build_function(f, &[]).expect("ssa build");
    super::verify::verify(&ssa).expect("ssa verify");
}

fn int(n: i64) -> HirExpr {
    HirExpr::Int(n)
}

fn param(i: u32) -> HirExpr {
    HirExpr::Var(HirBinding::Param(i))
}

fn local(id: u32) -> HirExpr {
    HirExpr::Var(HirBinding::Local(LocalId(id)))
}

fn add(lhs: HirExpr, rhs: HirExpr) -> HirExpr {
    HirExpr::Binary {
        op: HirBinOp::Add,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        ty: HirType::Int,
    }
}

fn cmp(op: HirBinOp, lhs: HirExpr, rhs: HirExpr) -> HirExpr {
    HirExpr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        ty: HirType::Bool,
    }
}

fn let_local(id: u32, value: HirExpr) -> HirStmt {
    HirStmt::Let {
        local: LocalId(id),
        value,
        ty: HirType::Int,
    }
}

fn assign_local(id: u32, value: HirExpr) -> HirStmt {
    HirStmt::Assign {
        target: HirBinding::Local(LocalId(id)),
        value,
    }
}

fn if_stmt(test: HirExpr, then_body: Vec<HirStmt>, else_body: Vec<HirStmt>) -> HirStmt {
    HirStmt::If {
        test,
        then_body,
        else_body,
    }
}

mod cases_a;
mod cases_b;
