use crate::checker::ExprInfo;
use crate::types::Type;
use crate::{checker::Checker, SymbolId};
use std::sync::Arc;
use varn_core::ast::operators::BinaryOp;

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

pub(super) fn closest_in_list<'a>(name: &str, candidates: &'a [Arc<str>]) -> Option<&'a str> {
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
    *ty
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

impl<'r> Checker<'r> {
    pub(crate) fn is_subclass_or_same(
        &self,
        candidate: &str,
        target: &str,
        bind: &crate::binder::BindResult,
    ) -> bool {
        let mut visited: Vec<String> = Vec::new();
        let mut current = candidate.to_string();
        loop {
            if current == target {
                return true;
            }
            if visited.iter().any(|v| v == &current) {
                return false;
            }
            visited.push(current.clone());
            match self.class_parent_step(&current, bind) {
                Some(next) => current = next,
                None => return false,
            }
        }
    }

    fn class_parent_step(&self, name: &str, bind: &crate::binder::BindResult) -> Option<String> {
        if let Some(parent) = bind.get_class_parent(name) {
            return Some(parent.to_string());
        }
        for spec in varn_modules::std_module_ids() {
            if let Some(rb) = self.resolver.stdlib_bind(spec) {
                if let Some(parent) = rb.class_parents.get(name) {
                    return Some(parent.to_string());
                }
            }
        }
        None
    }

    /// A value may be thrown only when it is (or could be) an `Error`
    /// subclass. `dynamic` stays throwable so untyped values (FFI, isolate
    /// payloads) don't cascade into throw errors.
    pub(crate) fn is_throwable(&self, ty: &Type, bind: &crate::binder::BindResult) -> bool {
        match self.ty_table.get(ty.0) {
            varn_core::TypeKind::Named(name, _) => {
                self.is_subclass_or_same(bind.interner.resolve(name), "Error", bind)
            }
            varn_core::TypeKind::Generic(name, _, _) => {
                self.is_subclass_or_same(bind.interner.resolve(name), "Error", bind)
            }
            varn_core::TypeKind::Union(list) => self
                .ty_table
                .get_list(list)
                .iter()
                .all(|id| self.is_throwable(&Type(*id, false), bind)),
            varn_core::TypeKind::This => self
                .current_class
                .as_deref()
                .is_some_and(|c| self.is_subclass_or_same(c, "Error", bind)),
            _ => ty.is_dynamic(),
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
        self.symbol_types.insert(symbol_id, ty);
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
