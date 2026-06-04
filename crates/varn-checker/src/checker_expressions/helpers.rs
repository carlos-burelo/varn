use crate::checker::ExprInfo;
use crate::types::Type;
use crate::{checker::Checker, SymbolId};
use std::rc::Rc;
use varn_core::ast::operators::BinaryOp;
use varn_core::TypeKind;

pub(super) fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut row: Vec<usize> = (0..=m).collect();
    for i in 1..=n {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=m {
            let next = row[j];
            row[j] = if a[i - 1] == b[j - 1] {
                prev
            } else {
                1 + prev.min(row[j]).min(row[j - 1])
            };
            prev = next;
        }
    }
    row[m]
}

pub(super) fn closest_in_list<'a>(name: &str, candidates: &'a [Rc<str>]) -> Option<&'a str> {
    let threshold = (name.len().max(1) / 3).max(1);
    candidates
        .iter()
        .filter_map(|c| {
            let d = levenshtein(name, c.as_ref());
            if d <= threshold {
                Some((d, c.as_ref()))
            } else {
                None
            }
        })
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

pub(super) fn base_type(ty: &Type) -> Type {
    match &ty.0 {
        TypeKind::LiteralInt(_) => Type::Int,
        TypeKind::LiteralFloat(_) => Type::Float,
        TypeKind::LiteralStr(_) => Type::Str,
        TypeKind::LiteralBool(_) => Type::Bool,
        _ => ty.clone(),
    }
}

pub(super) fn op_str(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Pow => "**",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
        BinaryOp::UShr => ">>>",
        BinaryOp::Eq => "==",
        BinaryOp::NotEq => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::LtEq => "<=",
        BinaryOp::GtEq => ">=",
        BinaryOp::In => "in",
        BinaryOp::Instanceof => "instanceof",
    }
}

impl Checker {
    pub(crate) fn is_subclass_or_same(
        &self,
        candidate: &str,
        target: &str,
        bind: &crate::binder::BindResult,
    ) -> bool {
        let mut current = candidate;
        loop {
            if current == target {
                return true;
            }
            match bind.get_class_parent(current) {
                Some(parent) => current = parent,
                None => return false,
            }
        }
    }

    pub(crate) fn record_type(&mut self, offset: u32, ty: Type) {
        if self.record_expr_types {
            self.expr_types.insert(
                offset,
                ExprInfo {
                    ty,
                    symbol_id: None,
                },
            );
        }
    }

    pub(crate) fn record_type_with_symbol(&mut self, offset: u32, ty: Type, symbol_id: SymbolId) {
        self.symbol_types.insert(symbol_id, ty.clone());
        self.mark_infer_env_dirty();
        if self.record_expr_types {
            self.expr_types.insert(
                offset,
                ExprInfo {
                    ty,
                    symbol_id: Some(symbol_id),
                },
            );
        }
    }

    pub(crate) fn record_member_type(&mut self, offset: u32, ty: Type, symbol_id: SymbolId) {
        if self.record_expr_types {
            self.expr_types.insert(
                offset,
                ExprInfo {
                    ty,
                    symbol_id: Some(symbol_id),
                },
            );
        }
    }
}
