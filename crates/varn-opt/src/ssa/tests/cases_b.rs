use super::*;
use crate::hir::{
    HirArrayEl, HirBinOp, HirCaseTest, HirExpr, HirMatchCase, HirObjectProp, HirPropKey, HirStmt,
    HirSwitchCase, HirTemplatePart, HirType, HirTypeTest, LocalId,
};

#[test]
fn method_call_lowers_to_callmethod() {
    // f(o, p) { return o.m(p) }
    let f = func(
        vec![HirType::Ref, HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::MethodCall {
            recv: Box::new(param(0)),
            name: "m".into(),
            args: vec![param(1)],
            ty: HirType::Int,
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: ref, v1: int):
    v2 = callmethod v0.m(v1)
    return v2
"
    );
}

#[test]
fn ternary_lowers_to_branch_and_result_phi() {
    // f(n) { return n > 0 ? 1 : 2 }
    let f = func(
        vec![HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::Conditional {
            test: Box::new(cmp(HirBinOp::Gt, param(0), int(0))),
            cons: Box::new(int(1)),
            alt: Box::new(int(2)),
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 0
    v2 = gt.bool v0, v1
    branch v2, b1, b2
  b1():
    v3 = int 1
    jump b3(v3)
  b2():
    v4 = int 2
    jump b3(v4)
  b3(v5: dyn):
    return v5
"
    );
}

#[test]
fn array_literal_lowers_to_buildarray() {
    // f(a, b) { return [a, b] }
    let f = func(
        vec![HirType::Int, HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::Array(vec![
            HirArrayEl::Expr(param(0)),
            HirArrayEl::Expr(param(1)),
        ])))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int, v1: int):
    v2 = array(v0, v1)
    return v2
"
    );
}

#[test]
fn object_literal_lowers_to_buildobject() {
    // f(x) { return { k: x } }
    let f = func(
        vec![HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::Object {
            properties: vec![HirObjectProp::Property {
                key: HirPropKey::Static("k".into()),
                value: param(0),
            }],
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = object {k: v0}
    return v1
"
    );
}

#[test]
fn template_lowers_to_buildstr() {
    // f(x) { return `a${x}` }
    let f = func(
        vec![HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::Template(vec![
            HirTemplatePart::Str("a".into()),
            HirTemplatePart::Expr(param(0)),
        ])))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = str \"a\"
    v2 = tostring v0
    v3 = buildstr(v1, v2)
    return v3
"
    );
}

#[test]
fn capture_free_closure_lowers_to_makeclosure() {
    // f() { return <inner fn> }  — capture-free → LoadStaticFn.
    let inner = func(vec![], 0, vec![HirStmt::Return(Some(int(0)))]);
    let f = func(
        vec![],
        0,
        vec![HirStmt::Return(Some(HirExpr::Closure {
            func: Box::new(inner),
            upvalues: vec![],
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0():
    v0 = closure f
    return v0
"
    );
}

#[test]
fn intrinsic_call_lowers_to_intrinsic() {
    // f(x) { return <intrinsic #5>(x) }
    let f = func(
        vec![HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::IntrinsicCall {
            object: Box::new(param(0)),
            args: vec![],
            wire_byte: 5,
            ty: HirType::Int,
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = intrinsic#5 v0
    return v1
"
    );
}

#[test]
fn nonnull_lowers_to_assertnotnull() {
    // f(x) { return x! }
    let f = func(
        vec![HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::NonNull(Box::new(param(0)))))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    assertnotnull v0
    return v0
"
    );
}

#[test]
fn sequence_evaluates_all_yields_last() {
    // f(a, b) { return (a + 1, b) }
    let f = func(
        vec![HirType::Int, HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::Sequence(vec![
            add(param(0), int(1)),
            param(1),
        ])))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int, v1: int):
    v2 = int 1
    v3 = add.int v0, v2
    return v1
"
    );
}

#[test]
fn decimal_literal_lowers_to_constdecimal() {
    // f() { return 3.14d }
    let f = func(
        vec![],
        0,
        vec![HirStmt::Return(Some(HirExpr::Decimal(
            rust_decimal::Decimal::new(314, 2),
        )))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0():
    v0 = decimal 3.14
    return v0
"
    );
}

#[test]
fn try_op_lowers_to_enumtag_branch_return() {
    // f(x) { return x? }
    let f = func(
        vec![HirType::Dynamic],
        0,
        vec![HirStmt::Return(Some(HirExpr::TryOp(Box::new(param(0)))))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: dyn):
    v1 = enumtag v0
    branch v1, b1, b2
  b1():
    return v0
  b2():
    return v0
"
    );
}

#[test]
fn type_test_typeof_lowers_to_typeof_eq() {
    // f(x) { return x is int }
    let f = func(
        vec![HirType::Dynamic],
        0,
        vec![HirStmt::Return(Some(HirExpr::TypeTest {
            value: Box::new(param(0)),
            kind: HirTypeTest::TypeofEq("int".into()),
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: dyn):
    v1 = typeof.str v0
    v2 = str \"int\"
    v3 = eq.dyn v1, v2
    return v3
"
    );
}

#[test]
fn this_lowers_to_move_from_reg0() {
    // f() { return this }
    let f = func(
        vec![],
        0,
        vec![HirStmt::Return(Some(HirExpr::This))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0():
    v0 = this
    return v0
"
    );
}

#[test]
fn throw_terminates_block() {
    // f(x) { throw x }
    let f = func(
        vec![HirType::Dynamic],
        0,
        vec![HirStmt::Throw(param(0))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: dyn):
    throw v0
"
    );
}

#[test]
fn range_lowers_to_range_inst() {
    // f(a, b) { return a..b }
    let f = func(
        vec![HirType::Int, HirType::Int],
        0,
        vec![HirStmt::Return(Some(HirExpr::Range {
            start: Box::new(param(0)),
            end: Box::new(param(1)),
            inclusive: false,
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int, v1: int):
    v2 = range v0..v1
    return v2
"
    );
}

#[test]
fn for_in_lowers_to_objectkeys_index_loop() {
    // f(o) { for (let k in o) {} return 0 }
    let f = func(
        vec![HirType::Dynamic],
        1,
        vec![
            HirStmt::ForIn {
                var: LocalId(0),
                object: param(0),
                body: vec![],
            },
            HirStmt::Return(Some(int(0))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: dyn):
    v1 = objectkeys v0
    v2 = int 0
    jump b1(v2)
  b1(v4: int):
    v3 = getprop v1.length
    v5 = lt.bool v4, v3
    branch v5, b2, b4
  b2():
    v6 = getindex v1[v4]
    jump b3
  b3():
    v7 = int 1
    v8 = add.int v4, v7
    jump b1(v8)
  b4():
    v9 = int 0
    return v9
"
    );
}

#[test]
fn switch_lowers_to_test_chain_and_bodies() {
    // f(x) { switch (x) { case 1: return 1; default: return 0 } }
    let f = func(
        vec![HirType::Int],
        0,
        vec![HirStmt::Switch {
            disc: param(0),
            cases: vec![
                HirSwitchCase {
                    test: Some(int(1)),
                    body: vec![HirStmt::Return(Some(int(1)))],
                },
                HirSwitchCase {
                    test: None,
                    body: vec![HirStmt::Return(Some(int(0)))],
                },
            ],
        }],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 1
    v2 = eq.dyn v0, v1
    branch v2, b1, b4
  b1():
    v3 = int 1
    return v3
  b2():
    v4 = int 0
    return v4
  b3():
    return
  b4():
    jump b2
"
    );
}

#[test]
fn for_of_lowers_to_iterator_protocol() {
    // f(a) { for (let x of a) {} return 0 }
    let f = func(
        vec![HirType::Dynamic],
        1,
        vec![
            HirStmt::ForOf {
                var: LocalId(0),
                iterable: param(0),
                body: vec![],
                is_await: false,
            },
            HirStmt::Return(Some(int(0))),
        ],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: dyn):
    v1 = getsymbol v0.@@iterator
    v2 = itercall v1(v0)
    jump b1
  b1():
    v3 = getprop v2.next
    v4 = itercall v3(v2)
    v5 = getprop v4.done
    branch v5, b3, b2
  b2():
    v6 = getprop v4.value
    jump b1
  b3():
    v7 = int 0
    return v7
"
    );
}

#[test]
fn match_lowers_to_test_chain_and_result_phi() {
    // f(n) { return match n { 0 => 1, x => 2 } }
    let f = func(
        vec![HirType::Int],
        1,
        vec![HirStmt::Return(Some(HirExpr::Match {
            subject: Box::new(param(0)),
            cases: vec![
                HirMatchCase {
                    test: HirCaseTest::Literal(int(0)),
                    guard: None,
                    body: vec![],
                    result: Some(int(1)),
                },
                HirMatchCase {
                    test: HirCaseTest::Bind(LocalId(0)),
                    guard: None,
                    body: vec![],
                    result: Some(int(2)),
                },
            ],
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0(v0: int):
    v1 = int 0
    v2 = eq.dyn v0, v1
    branch v2, b2, b3
  b1(v5: dyn):
    return v5
  b2():
    v3 = int 1
    jump b1(v3)
  b3():
    jump b4
  b4():
    v4 = int 2
    jump b1(v4)
"
    );
}

#[test]
fn super_method_call_lowers_to_supercall() {
    // f() { return super.greet(7) }
    let f = func(
        vec![],
        0,
        vec![HirStmt::Return(Some(HirExpr::SuperMethodCall {
            name: "greet".into(),
            args: vec![int(7)],
        }))],
    );
    assert_eq!(
        build(&f),
        "\
fn f:
  b0():
    v0 = int 7
    v1 = supercall super.greet(v0)
    return v1
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
