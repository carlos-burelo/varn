use rustc_hash::FxHashSet;

use crate::hir::{
    HirArrayEl, HirAssignTarget, HirBinding, HirExpr, HirFunction, HirObjectProp,
    HirOptionalProperty, HirPropKey, HirStmt, HirTemplatePart, HirUpvalueSrc,
};
use super::super::ir::VarId;

pub(super) fn scan_pinned_vars(func: &HirFunction) -> FxHashSet<VarId> {
    let mut pinned = FxHashSet::default();
    for param in &func.params {
        if let Some(d) = &param.default {
            scan_expr(d, &mut pinned, false);
        }
    }
    for s in &func.body {
        scan_stmt(s, &mut pinned, false);
    }
    pinned
}

fn scan_stmt(stmt: &HirStmt, pinned: &mut FxHashSet<VarId>, in_try: bool) {
    match stmt {
        HirStmt::Expr(expr) => scan_expr(expr, pinned, in_try),
        HirStmt::Let { value, .. } => scan_expr(value, pinned, in_try),
        HirStmt::Assign { target, value } => {
            scan_expr(value, pinned, in_try);
            if in_try {
                match target {
                    HirBinding::Local(id) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirBinding::Param(i) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
        }
        HirStmt::SetMember { object, value, .. } | HirStmt::SetFixedField { object, value, .. } => {
            scan_expr(object, pinned, in_try);
            scan_expr(value, pinned, in_try);
        }
        HirStmt::SetIndex {
            object,
            index,
            value,
            ..
        } => {
            scan_expr(object, pinned, in_try);
            scan_expr(index, pinned, in_try);
            scan_expr(value, pinned, in_try);
        }
        HirStmt::Return(value) => {
            if let Some(e) = value {
                scan_expr(e, pinned, in_try);
            }
        }
        HirStmt::Throw(expr) => scan_expr(expr, pinned, in_try),
        HirStmt::If {
            test,
            then_body,
            else_body,
        } => {
            scan_expr(test, pinned, in_try);
            for s in then_body {
                scan_stmt(s, pinned, in_try);
            }
            for s in else_body {
                scan_stmt(s, pinned, in_try);
            }
        }
        HirStmt::While { test, body } => {
            scan_expr(test, pinned, in_try);
            for s in body {
                scan_stmt(s, pinned, in_try);
            }
        }
        HirStmt::ForClassic { test, update, body } => {
            scan_expr(test, pinned, in_try);
            for s in update {
                scan_stmt(s, pinned, in_try);
            }
            for s in body {
                scan_stmt(s, pinned, in_try);
            }
        }
        HirStmt::ForOf { iterable, body, .. } => {
            scan_expr(iterable, pinned, in_try);
            for s in body {
                scan_stmt(s, pinned, in_try);
            }
        }
        HirStmt::ForIn { object, body, .. } => {
            scan_expr(object, pinned, in_try);
            for s in body {
                scan_stmt(s, pinned, in_try);
            }
        }
        HirStmt::DoWhile { body, test } => {
            for s in body {
                scan_stmt(s, pinned, in_try);
            }
            scan_expr(test, pinned, in_try);
        }
        HirStmt::Switch { disc, cases } => {
            scan_expr(disc, pinned, in_try);
            for c in cases {
                if let Some(t) = &c.test {
                    scan_expr(t, pinned, in_try);
                }
                for s in &c.body {
                    scan_stmt(s, pinned, in_try);
                }
            }
        }
        HirStmt::Try {
            block,
            catches,
            finally,
        } => {
            for s in block {
                scan_stmt(s, pinned, true);
            }
            for c in catches {
                for s in &c.body {
                    scan_stmt(s, pinned, in_try);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    scan_stmt(s, pinned, in_try);
                }
            }
        }
        HirStmt::Dispose { target, .. } => {
            pinned.insert(VarId::Local(*target));
        }
        _ => {}
    }
}

fn scan_expr(expr: &HirExpr, pinned: &mut FxHashSet<VarId>, in_try: bool) {
    match expr {
        HirExpr::NonNull(inner) => scan_expr(inner, pinned, in_try),
        HirExpr::TryOp(inner) => scan_expr(inner, pinned, in_try),
        HirExpr::TypeTest { value, .. } => scan_expr(value, pinned, in_try),
        HirExpr::Sequence(exprs) => {
            for e in exprs {
                scan_expr(e, pinned, in_try);
            }
        }
        HirExpr::Range { start, end, .. } => {
            scan_expr(start, pinned, in_try);
            scan_expr(end, pinned, in_try);
        }
        HirExpr::Template(parts) => {
            for p in parts {
                if let HirTemplatePart::Expr(e) = p {
                    scan_expr(e, pinned, in_try);
                }
            }
        }
        HirExpr::Assign { target, value } => {
            scan_expr(value, pinned, in_try);
            if in_try {
                match &**target {
                    HirAssignTarget::Var(HirBinding::Local(id)) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirAssignTarget::Var(HirBinding::Param(i)) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            scan_expr(lhs, pinned, in_try);
            scan_expr(rhs, pinned, in_try);
        }
        HirExpr::Unary { operand, .. } => scan_expr(operand, pinned, in_try),
        HirExpr::Call { callee, args, .. } => {
            scan_expr(callee, pinned, in_try);
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::SelfCall { args, .. } => {
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::Member { object, .. } | HirExpr::GetFixedField { object, .. } => {
            scan_expr(object, pinned, in_try)
        }
        HirExpr::Index { object, index, .. } => {
            scan_expr(object, pinned, in_try);
            scan_expr(index, pinned, in_try);
        }
        HirExpr::MethodCall { recv, args, .. } => {
            scan_expr(recv, pinned, in_try);
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::SuperCall { args } => {
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::SuperMethodCall { args, .. } => {
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::ExtensionCall { recv, args, .. } => {
            scan_expr(recv, pinned, in_try);
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::Class(cls) => {
            if let Some(sup) = &cls.super_class {
                scan_expr(sup, pinned, in_try);
            }
            for (_, init) in &cls.static_fields {
                if let Some(init) = init {
                    scan_expr(init, pinned, in_try);
                }
            }
            for deco in &cls.decorators {
                scan_expr(deco, pinned, in_try);
            }
            for uv in &cls.ctor.upvalues {
                match uv {
                    HirUpvalueSrc::ParentLocal(id) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirUpvalueSrc::ParentParam(i) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
            for deco in &cls.ctor.decorators {
                scan_expr(deco, pinned, in_try);
            }
            for m in &cls.methods {
                for uv in &m.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
                for deco in &m.decorators {
                    scan_expr(deco, pinned, in_try);
                }
            }
            for m in &cls.static_methods {
                for uv in &m.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
                for deco in &m.decorators {
                    scan_expr(deco, pinned, in_try);
                }
            }
            for a in &cls.getters {
                for uv in &a.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for a in &cls.setters {
                for uv in &a.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for b in &cls.static_blocks {
                for uv in &b.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
        }
        HirExpr::Enum(en) => {
            for (_, init) in &en.static_fields {
                if let Some(init) = init {
                    scan_expr(init, pinned, in_try);
                }
            }
            for v in &en.variants {
                for arg in &v.const_args {
                    scan_expr(arg, pinned, in_try);
                }
            }
            for uv in &en.ctor.upvalues {
                match uv {
                    HirUpvalueSrc::ParentLocal(id) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirUpvalueSrc::ParentParam(i) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
            for m in &en.methods {
                for uv in &m.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for m in &en.static_methods {
                for uv in &m.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for a in &en.getters {
                for uv in &a.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for a in &en.setters {
                for uv in &a.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
            for b in &en.static_blocks {
                for uv in &b.upvalues {
                    match uv {
                        HirUpvalueSrc::ParentLocal(id) => {
                            pinned.insert(VarId::Local(*id));
                        }
                        HirUpvalueSrc::ParentParam(i) => {
                            pinned.insert(VarId::Param(*i));
                        }
                        _ => {}
                    }
                }
            }
        }
        HirExpr::Closure { upvalues, .. } => {
            for uv in upvalues {
                match uv {
                    HirUpvalueSrc::ParentLocal(id) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirUpvalueSrc::ParentParam(i) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
        }
        HirExpr::Update { target, .. } => {
            if in_try {
                match &**target {
                    HirAssignTarget::Var(HirBinding::Local(id)) => {
                        pinned.insert(VarId::Local(*id));
                    }
                    HirAssignTarget::Var(HirBinding::Param(i)) => {
                        pinned.insert(VarId::Param(*i));
                    }
                    _ => {}
                }
            }
        }
        HirExpr::Array(els) => {
            for el in els {
                match el {
                    HirArrayEl::Expr(e) => scan_expr(e, pinned, in_try),
                    HirArrayEl::Spread(e) => scan_expr(e, pinned, in_try),
                    HirArrayEl::Hole => {}
                }
            }
        }
        HirExpr::Object { properties } => {
            for p in properties {
                match p {
                    HirObjectProp::Property { key, value } => {
                        if let HirPropKey::Computed(e) = key {
                            scan_expr(e, pinned, in_try);
                        }
                        scan_expr(value, pinned, in_try);
                    }
                    HirObjectProp::Method { key, upvalues, .. } => {
                        if let HirPropKey::Computed(e) = key {
                            scan_expr(e, pinned, in_try);
                        }
                        for uv in upvalues {
                            match uv {
                                HirUpvalueSrc::ParentLocal(id) => {
                                    pinned.insert(VarId::Local(*id));
                                }
                                HirUpvalueSrc::ParentParam(i) => {
                                    pinned.insert(VarId::Param(*i));
                                }
                                _ => {}
                            }
                        }
                    }
                    HirObjectProp::Spread(e) => scan_expr(e, pinned, in_try),
                }
            }
        }
        HirExpr::Match { subject, cases } => {
            scan_expr(subject, pinned, in_try);
            for c in cases {
                if let Some(g) = &c.guard {
                    scan_expr(g, pinned, in_try);
                }
                for s in &c.body {
                    scan_stmt(s, pinned, in_try);
                }
                if let Some(r) = &c.result {
                    scan_expr(r, pinned, in_try);
                }
            }
        }
        HirExpr::OptionalChain { object, property } => {
            scan_expr(object, pinned, in_try);
            match property {
                HirOptionalProperty::Index(e) => scan_expr(e, pinned, in_try),
                HirOptionalProperty::Call(args) => {
                    for a in args {
                        scan_expr(a, pinned, in_try);
                    }
                }
                HirOptionalProperty::MethodCall(_, args) => {
                    for a in args {
                        scan_expr(a, pinned, in_try);
                    }
                }
                _ => {}
            }
        }
        HirExpr::Await(e) => scan_expr(e, pinned, in_try),
        HirExpr::Spawn(e) => scan_expr(e, pinned, in_try),
        HirExpr::Yield(e) => scan_expr(e, pinned, in_try),
        HirExpr::IntrinsicCall { object, args, .. } => {
            scan_expr(object, pinned, in_try);
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::NativeMethodCall { object, args, .. } => {
            scan_expr(object, pinned, in_try);
            for a in args {
                scan_expr(a, pinned, in_try);
            }
        }
        HirExpr::ModuleSlot { object, .. } => scan_expr(object, pinned, in_try),
        HirExpr::ObjectRest { object, .. } => scan_expr(object, pinned, in_try),
        _ => {}
    }
}
