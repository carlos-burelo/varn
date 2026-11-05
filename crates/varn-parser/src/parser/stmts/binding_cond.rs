use crate::error::PResult;
use crate::stream::TokenStream;
use varn_core::ast::operators::{BinaryOp, VarKind};
use varn_core::ast::{
    Decl, ExprId, ExprKind, Pattern, StmtId, StmtKind, VarDeclarator, VariableDecl,
};
use varn_core::{Atom, SourceRange, TokenKind};

pub(super) struct Binding {
    kind: VarKind,
    pattern: Pattern,
    source: ExprId,
    range: SourceRange,
}

pub(super) fn parse_binding(s: &mut TokenStream) -> PResult<Option<Binding>> {
    let kind = if s.check(TokenKind::Let) {
        VarKind::Let
    } else if s.check(TokenKind::Const) {
        VarKind::Const
    } else {
        return Ok(None);
    };
    let start = s.range();
    s.advance();
    let pattern = super::super::patterns::parse_pattern(s)?;
    s.expect(TokenKind::Eq)?;
    let source = crate::expressions::parse_expr(s)?;
    let range = s.span_from(start);
    Ok(Some(Binding {
        kind,
        pattern,
        source,
        range,
    }))
}

fn variable(
    s: &mut TokenStream,
    range: SourceRange,
    kind: VarKind,
    id: Pattern,
    init: ExprId,
) -> StmtId {
    let decl = VariableDecl {
        kind,
        ast_id: s.next_ast_id(),
        declarators: vec![VarDeclarator {
            id,
            type_ann: None,
            init: Some(init),
            range,
        }],
        is_declare: false,
        doc: None,
        range,
    };
    s.stmt(range, StmtKind::Decl(Box::new(Decl::Variable(decl))))
}

struct Pieces {
    source_decl: StmtId,
    bound_decl: StmtId,
    present: ExprId,
    absent: ExprId,
}

fn pieces(s: &mut TokenStream, binding: Binding) -> Pieces {
    let range = binding.range;
    let tmp: Atom = s.interner.intern(varn_core::intrinsics::BOUND_SOURCE);
    let tmp_pattern = Pattern::Identifier { name: tmp, range };
    let source_decl = variable(s, range, VarKind::Const, tmp_pattern, binding.source);
    let tmp_ref = s.expr(range, ExprKind::Identifier { name: tmp });
    let bound_decl = variable(s, range, binding.kind, binding.pattern, tmp_ref);
    let compare = |s: &mut TokenStream, op: BinaryOp| {
        let left = s.expr(range, ExprKind::Identifier { name: tmp });
        let right = s.expr(range, ExprKind::NullLiteral);
        s.expr(range, ExprKind::Binary { op, left, right })
    };
    let present = compare(s, BinaryOp::NotEq);
    let absent = compare(s, BinaryOp::Eq);
    Pieces {
        source_decl,
        bound_decl,
        present,
        absent,
    }
}

pub(super) fn desugar_if(
    s: &mut TokenStream,
    binding: Binding,
    consequent: StmtId,
    alternate: Option<StmtId>,
    range: SourceRange,
) -> StmtId {
    let p = pieces(s, binding);
    let then_block = s.stmt(
        range,
        StmtKind::Block {
            stmts: vec![p.bound_decl, consequent],
        },
    );
    let branch = s.stmt(
        range,
        StmtKind::If {
            test: p.present,
            consequent: then_block,
            alternate,
        },
    );
    s.stmt(
        range,
        StmtKind::Block {
            stmts: vec![p.source_decl, branch],
        },
    )
}

pub(super) fn desugar_while(
    s: &mut TokenStream,
    binding: Binding,
    body: StmtId,
    range: SourceRange,
) -> StmtId {
    let p = pieces(s, binding);
    let leave = s.stmt(range, StmtKind::Break { label: None });
    let guard = s.stmt(
        range,
        StmtKind::If {
            test: p.absent,
            consequent: leave,
            alternate: None,
        },
    );
    let iteration = s.stmt(
        range,
        StmtKind::Block {
            stmts: vec![p.source_decl, guard, p.bound_decl, body],
        },
    );
    let forever = s.expr(range, ExprKind::BoolLiteral { value: true });
    s.stmt(
        range,
        StmtKind::While {
            test: forever,
            body: iteration,
        },
    )
}
