use super::super::Checker;
use super::assigned_targets::{assigned_in_expr, assigned_in_stmt, AssignedTargets, MemberRoot};
use crate::checker::recorder::Recorder;
use varn_core::ast::{ArrowBody, ExprId, ExprKind, Pattern, StmtId};
use varn_core::Atom;
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolId;

use super::reassignment::MemberKey;

pub(crate) enum ClosureBodyRef {
    Stmt(StmtId),
    Expr(ExprId),
}

#[derive(Clone, Default)]
pub(crate) struct ClosureEffects {
    pub vars: Vec<SymbolId>,
    pub members: Vec<MemberKey>,
}

impl<'r> Checker<'r> {
    pub(crate) fn closure_assigned_symbols(
        &mut self,
        body: ClosureBodyRef,
        bind: &BindResult,
    ) -> ClosureEffects {
        let mut targets = AssignedTargets::empty();
        match body {
            ClosureBodyRef::Stmt(stmt) => assigned_in_stmt(stmt, self.ast_arena, &mut targets),
            ClosureBodyRef::Expr(expr) => assigned_in_expr(expr, self.ast_arena, &mut targets),
        }
        let mut ids: Vec<SymbolId> = targets
            .vars
            .into_iter()
            .filter_map(|name| {
                bind.scopes
                    .get(self.current_scope)
                    .resolve(name, &bind.scopes)
            })
            .collect();
        ids.sort();
        ids.dedup();
        let mut members = Vec::new();
        for (root, path) in targets.members {
            let resolved = match root {
                MemberRoot::This(_) => Some(None),
                MemberRoot::Name(_, name) => bind
                    .scopes
                    .get(self.current_scope)
                    .resolve(name, &bind.scopes)
                    .map(Some),
            };
            let Some(id) = resolved else {
                continue;
            };
            members.push((id, Self::atoms_to_path(&path, bind)));
        }
        members.sort();
        members.dedup();
        ClosureEffects { vars: ids, members }
    }

    pub(crate) fn closure_def_effects(
        &mut self,
        expr: ExprId,
        bind: &BindResult,
    ) -> Option<ClosureEffects> {
        let body = match &self.ast_arena.expr(expr).kind {
            ExprKind::Function { body, .. } => ClosureBodyRef::Stmt(*body),
            ExprKind::Arrow { body, .. } => match body.as_ref() {
                ArrowBody::Block(stmt) => ClosureBodyRef::Stmt(*stmt),
                ArrowBody::Expr(value) => ClosureBodyRef::Expr(*value),
            },
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Identifier { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => return None,
        };
        Some(self.closure_assigned_symbols(body, bind))
    }

    fn forget_effects(&mut self, rec: &Recorder, effects: &ClosureEffects, bind: &BindResult) {
        for id in &effects.vars {
            self.forget_symbol(rec, *id, bind);
        }
        for key in &effects.members {
            if self.member_narrowed.remove(key).is_some() {
                self.mark_infer_env_dirty();
            }
        }
    }

    pub(crate) fn forget_closure_args(
        &mut self,
        rec: &Recorder,
        args: &[varn_core::ast::Arg],
        bind: &BindResult,
    ) {
        for arg in args {
            let value = match arg {
                varn_core::ast::Arg::Positional(value)
                | varn_core::ast::Arg::Named { value, .. } => *value,
                varn_core::ast::Arg::Spread(_) => continue,
            };
            if let Some(effects) = self.closure_def_effects(value, bind) {
                self.forget_effects(rec, &effects, bind);
            }
        }
    }

    pub(crate) fn forget_called_closure(
        &mut self,
        rec: &Recorder,
        callee: ExprId,
        callee_sid: Option<SymbolId>,
        args: &[varn_core::ast::Arg],
        bind: &BindResult,
    ) {
        if let Some(sid) = callee_sid {
            if let Some(effects) = self.closure_effects.get(&sid).cloned() {
                self.forget_effects(rec, &effects, bind);
            }
        } else if let Some(effects) = self.closure_def_effects(callee, bind) {
            self.forget_effects(rec, &effects, bind);
        }
        self.forget_closure_args(rec, args, bind);
    }

    pub(crate) fn pattern_bound_atoms(pattern: &Pattern, out: &mut Vec<Atom>) {
        match pattern {
            Pattern::Identifier { name, .. } => out.push(*name),
            Pattern::Array { elements, rest, .. } => {
                for el in elements.iter().flatten() {
                    Self::pattern_bound_atoms(&el.pattern, out);
                }
                if let Some(rest) = rest {
                    Self::pattern_bound_atoms(rest, out);
                }
            }
            Pattern::Object {
                properties, rest, ..
            } => {
                for prop in properties {
                    Self::pattern_bound_atoms(&prop.value, out);
                }
                if let Some(rest) = rest {
                    Self::pattern_bound_atoms(rest, out);
                }
            }
            Pattern::Assignment { left, .. } => Self::pattern_bound_atoms(left, out),
            Pattern::Rest { argument, .. } => Self::pattern_bound_atoms(argument, out),
        }
    }
}
