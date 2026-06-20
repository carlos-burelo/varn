//! SSA construction tests: build from hand-written HIR, assert the dump.

use crate::hir::{
    HirBinOp, HirBinding, HirExpr, HirFunction, HirParam, HirStmt, HirType, LocalId,
};

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
    dump(&build_function(f).expect("ssa build"))
}

/// Build a function to SSA and assert the verifier accepts it.
fn verify_ok(f: &HirFunction) {
    let ssa = build_function(f).expect("ssa build");
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

#[test]
fn identity_returns_param() {
    let f = func(
        vec![HirType::Int],
        0,
        vec![HirStmt::Return(Some(param(0)))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    return v0
"
    );
}

#[test]
fn binary_add_param_const() {
    let f = func(
        vec![HirType::Int],
        0,
        vec![HirStmt::Return(Some(add(param(0), int(1))))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 1
    v2 = add.int v0, v1
    return v2
"
    );
}

fn if_stmt(test: HirExpr, then_body: Vec<HirStmt>, else_body: Vec<HirStmt>) -> HirStmt {
    HirStmt::If {
        test,
        then_body,
        else_body,
    }
}

#[test]
fn if_merge_inserts_block_param_phi() {
    // var y = 1; if (p) { y = 2 } return y
    let f = func(
        vec![HirType::Int],
        1,
        vec![
            HirStmt::Let {
                local: LocalId(0),
                value: int(1),
                ty: HirType::Int,
            },
            if_stmt(
                param(0),
                vec![HirStmt::Assign {
                    target: HirBinding::Local(LocalId(0)),
                    value: int(2),
                }],
                vec![],
            ),
            HirStmt::Return(Some(local(0))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 1
    branch v0, b1, b2(v1)
  b1():
    v2 = int 2
    jump b2(v2)
  b2(v3: int):
    return v3
"
    );
}

#[test]
fn if_else_both_assign_merges_both_values() {
    // var y = 0; if (p) { y = 1 } else { y = 2 } return y
    let f = func(
        vec![HirType::Int],
        1,
        vec![
            HirStmt::Let {
                local: LocalId(0),
                value: int(0),
                ty: HirType::Int,
            },
            if_stmt(
                param(0),
                vec![HirStmt::Assign {
                    target: HirBinding::Local(LocalId(0)),
                    value: int(1),
                }],
                vec![HirStmt::Assign {
                    target: HirBinding::Local(LocalId(0)),
                    value: int(2),
                }],
            ),
            HirStmt::Return(Some(local(0))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 0
    branch v0, b1, b3
  b1():
    v2 = int 1
    jump b2(v2)
  b2(v4: int):
    return v4
  b3():
    v3 = int 2
    jump b2(v3)
"
    );
}

#[test]
fn unmodified_var_across_if_has_no_phi() {
    // if (p) { } return q  — q is never reassigned, so the merge needs no phi
    let f = func(
        vec![HirType::Int, HirType::Int],
        0,
        vec![
            if_stmt(param(0), vec![], vec![]),
            HirStmt::Return(Some(param(1))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int, v1: int):
    branch v0, b1, b2
  b1():
    jump b2
  b2():
    return v1
"
    );
}

#[test]
fn local_reassign_reads_latest_value() {
    // var y = p + 1; y = y + 1; return y
    let f = func(
        vec![HirType::Int],
        1,
        vec![
            HirStmt::Let {
                local: LocalId(0),
                value: add(param(0), int(1)),
                ty: HirType::Int,
            },
            HirStmt::Assign {
                target: HirBinding::Local(LocalId(0)),
                value: add(local(0), int(1)),
            },
            HirStmt::Return(Some(local(0))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 1
    v2 = add.int v0, v1
    v3 = int 1
    v4 = add.int v2, v3
    return v4
"
    );
}

#[test]
fn while_loop_carries_induction_var_through_header_phi() {
    // var i = 0; while (i < n) { i = i + 1 } return i
    // The header is the only block with a phi (loop-carried `i`); `n` is
    // unchanged so its incomplete phi collapses to the entry param.
    let f = func(
        vec![HirType::Int],
        1,
        vec![
            let_local(0, int(0)),
            HirStmt::While {
                test: cmp(HirBinOp::Lt, local(0), param(0)),
                body: vec![assign_local(0, add(local(0), int(1)))],
            },
            HirStmt::Return(Some(local(0))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 0
    jump b1(v1)
  b1(v2: int):
    v4 = lt.bool v2, v0
    branch v4, b2, b3
  b2():
    v5 = int 1
    v6 = add.int v2, v5
    jump b1(v6)
  b3():
    return v2
"
    );
}

#[test]
fn for_classic_increments_in_update_block() {
    // var i = 0; for (; i < n; i = i + 1) {} return i
    let f = func(
        vec![HirType::Int],
        1,
        vec![
            let_local(0, int(0)),
            HirStmt::ForClassic {
                test: cmp(HirBinOp::Lt, local(0), param(0)),
                update: vec![assign_local(0, add(local(0), int(1)))],
                body: vec![],
            },
            HirStmt::Return(Some(local(0))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 0
    jump b1(v1)
  b1(v2: int):
    v4 = lt.bool v2, v0
    branch v4, b2, b4
  b2():
    jump b3
  b3():
    v5 = int 1
    v6 = add.int v2, v5
    jump b1(v6)
  b4():
    return v2
"
    );
}

#[test]
fn break_in_loop_jumps_to_exit() {
    // var i = 0; while (true) { if (i >= n) break; i = i + 1 } return i
    let f = func(
        vec![HirType::Int],
        1,
        vec![
            let_local(0, int(0)),
            HirStmt::While {
                test: HirExpr::Bool(true),
                body: vec![
                    if_stmt(
                        cmp(HirBinOp::Ge, local(0), param(0)),
                        vec![HirStmt::Break],
                        vec![],
                    ),
                    assign_local(0, add(local(0), int(1))),
                ],
            },
            HirStmt::Return(Some(local(0))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 0
    jump b1(v1)
  b1(v3: int):
    v2 = bool true
    branch v2, b2, b3
  b2():
    v5 = ge.bool v3, v0
    branch v5, b4, b5
  b3():
    return v3
  b4():
    jump b3
  b5():
    v6 = int 1
    v7 = add.int v3, v6
    jump b1(v7)
"
    );
}

#[test]
fn do_while_runs_body_before_test() {
    // var i = 0; do { i = i + 1 } while (i < n) return i
    // The body block is the loop entry and the test lives in a latch block.
    let f = func(
        vec![HirType::Int],
        1,
        vec![
            let_local(0, int(0)),
            HirStmt::DoWhile {
                body: vec![assign_local(0, add(local(0), int(1)))],
                test: cmp(HirBinOp::Lt, local(0), param(0)),
            },
            HirStmt::Return(Some(local(0))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 0
    jump b1(v1)
  b1(v2: int):
    v3 = int 1
    v4 = add.int v2, v3
    jump b2
  b2():
    v6 = lt.bool v4, v0
    branch v6, b1(v4), b3
  b3():
    return v4
"
    );
}

#[test]
fn continue_in_for_skips_rest_of_body_but_runs_update() {
    // var i = 0; var s = 0; for (; i < n; i = i + 1) { continue; s = s + 1 }
    // return s  — `continue` jumps to the update block, so `s` is never touched.
    let f = func(
        vec![HirType::Int],
        2,
        vec![
            let_local(0, int(0)),
            let_local(1, int(0)),
            HirStmt::ForClassic {
                test: cmp(HirBinOp::Lt, local(0), param(0)),
                update: vec![assign_local(0, add(local(0), int(1)))],
                body: vec![HirStmt::Continue, assign_local(1, add(local(1), int(1)))],
            },
            HirStmt::Return(Some(local(1))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 0
    v2 = int 0
    jump b1(v1)
  b1(v3: int):
    v5 = lt.bool v3, v0
    branch v5, b2, b4
  b2():
    jump b3
  b3():
    v6 = int 1
    v7 = add.int v3, v6
    jump b1(v7)
  b4():
    return v2
"
    );
}

#[test]
fn nested_if_fallthrough_merges_from_inner_block() {
    // var y = 0; if (p) { if (q) { y = 1 } y = y + 1 } return y
    // The outer then-branch falls through to the outer merge from the INNER
    // merge block, not from the outer then's first block — so the merge phi
    // must read `y` on the inner-merge edge (regression guard for the
    // edge-predecessor tracking).
    let f = func(
        vec![HirType::Int, HirType::Int],
        1,
        vec![
            let_local(0, int(0)),
            if_stmt(
                param(0),
                vec![
                    if_stmt(param(1), vec![assign_local(0, int(1))], vec![]),
                    assign_local(0, add(local(0), int(1))),
                ],
                vec![],
            ),
            HirStmt::Return(Some(local(0))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int, v1: int):
    v2 = int 0
    branch v0, b1, b2(v2)
  b1():
    branch v1, b3, b4(v2)
  b2(v7: int):
    return v7
  b3():
    v3 = int 1
    jump b4(v3)
  b4(v4: int):
    v5 = int 1
    v6 = add.int v4, v5
    jump b2(v6)
"
    );
}

#[test]
fn call_global_lowers_to_call_inst() {
    // f(n) { return g(n) }  — g is a module global.
    let f = func(
        vec![HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::Call {
            callee: Box::new(HirExpr::Var(HirBinding::Global("g".into()))),
            args: vec![param(0)],
            ty: HirType::Int,
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = global g
    v2 = call v1(v0)
    return v2
"
    );
}

#[test]
fn self_call_lowers_to_callself_inst() {
    // f(n) { return f(n) }  via SelfCall (statically-resolved self recursion).
    let f = func(
        vec![HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::SelfCall {
            args: vec![param(0)],
            ty: HirType::Int,
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = callself(v0)
    return v1
"
    );
}

#[test]
fn member_read_lowers_to_getproperty() {
    // f(o) { return o.x }
    let f = func(
        vec![HirType::Ref],
        0,
        vec![HirStmt::Return(Some(HirExpr::Member {
            object: Box::new(param(0)),
            name: "x".into(),
            ty: HirType::Int,
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: ref):
    v1 = getprop v0.x
    return v1
"
    );
}

#[test]
fn index_read_lowers_to_getindex() {
    // f(a, i) { return a[i] }
    let f = func(
        vec![HirType::Ref, HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::Index {
            object: Box::new(param(0)),
            index: Box::new(param(1)),
            ty: HirType::Int,
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: ref, v1: int):
    v2 = getindex v0[v1]
    return v2
"
    );
}

#[test]
fn member_write_lowers_to_setproperty() {
    // f(o) { o.x = 5; return o.x }
    let f = func(
        vec![HirType::Ref],
        0,
        vec![
            HirStmt::SetMember {
                object: param(0),
                name: "x".into(),
                value: int(5),
            },
            HirStmt::Return(Some(HirExpr::Member {
                object: Box::new(param(0)),
                name: "x".into(),
                ty: HirType::Int,
            })),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: ref):
    v1 = int 5
    setprop v0.x = v1
    v2 = getprop v0.x
    return v2
"
    );
}

#[test]
fn index_write_lowers_to_setindex() {
    // f(a, i, v) { a[i] = v; return a[i] }
    let f = func(
        vec![HirType::Ref, HirType::Int, HirType::Int],
        0,
        vec![
            HirStmt::SetIndex {
                object: param(0),
                index: param(1),
                value: param(2),
            },
            HirStmt::Return(Some(HirExpr::Index {
                object: Box::new(param(0)),
                index: Box::new(param(1)),
                ty: HirType::Int,
            })),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: ref, v1: int, v2: int):
    setindex v0[v1] = v2
    v3 = getindex v0[v1]
    return v3
"
    );
}

#[test]
fn verifier_accepts_constructed_ssa() {
    // The verifier must not reject any well-formed function the builder emits.
    let while_loop = func(
        vec![HirType::Int],
        1,
        vec![
            let_local(0, int(0)),
            HirStmt::While {
                test: cmp(HirBinOp::Lt, local(0), param(0)),
                body: vec![assign_local(0, add(local(0), int(1)))],
            },
            HirStmt::Return(Some(local(0))),
        ],
    );
    let nested = func(
        vec![HirType::Int, HirType::Int],
        1,
        vec![
            let_local(0, int(0)),
            if_stmt(
                param(0),
                vec![
                    if_stmt(param(1), vec![assign_local(0, int(1))], vec![]),
                    assign_local(0, add(local(0), int(1))),
                ],
                vec![],
            ),
            HirStmt::Return(Some(local(0))),
        ],
    );
    let break_loop = func(
        vec![HirType::Int],
        1,
        vec![
            let_local(0, int(0)),
            HirStmt::While {
                test: HirExpr::Bool(true),
                body: vec![
                    if_stmt(cmp(HirBinOp::Ge, local(0), param(0)), vec![HirStmt::Break], vec![]),
                    assign_local(0, add(local(0), int(1))),
                ],
            },
            HirStmt::Return(Some(local(0))),
        ],
    );
    verify_ok(&while_loop);
    verify_ok(&nested);
    verify_ok(&break_loop);
}
