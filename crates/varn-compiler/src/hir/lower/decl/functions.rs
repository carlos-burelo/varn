//! Function declarations and the shared body-lowering used by every callable
//! form (functions, methods, arrows).

use std::rc::Rc;
use varn_core::AnnKey;

use varn_core::ast::{Expr, FunctionDecl, Param, Pattern, StmtKind};

use super::super::*;

impl<'a> Lowerer<'a> {
    pub(in crate::hir::lower) fn lower_function(
        &mut self,
        f: &FunctionDecl,
        scope: &mut Scope,
    ) -> R<(HirFunction, Vec<HirUpvalueSrc>)> {
        let (mut func, upvalues) = self.lower_function_like(
            f.id.clone(),
            &f.params,
            f.modifiers.is_async,
            f.modifiers.is_generator,
            false,
            false,
            BodyRef::Block(&f.body),
            &[],
            scope,
        )?;
        // Declared return type, recorded by the checker at the function's
        // name offset.
        func.return_ty = self.value_ty(AnnKey::decl(f.id_offset));
        Ok((func, upvalues))
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::hir::lower) fn lower_function_like(
        &mut self,
        name: Rc<str>,
        params_ast: &[Param],
        is_async: bool,
        is_generator: bool,
        _generic: bool,
        has_this: bool,
        body: BodyRef<'_>,
        field_inits: &[(Rc<str>, &Expr)],
        scope: &mut Scope,
    ) -> R<(HirFunction, Vec<HirUpvalueSrc>)> {
        scope.push_frame();
        let prev_fn = self.current_fn.take();
        self.current_fn = Some(name.clone());

        let built = self.lower_function_body(params_ast, body, field_inits, scope);

        self.current_fn = prev_fn;
        let (params, mut body) = match built {
            Ok(v) => v,
            Err(e) => {
                scope.pop_frame();
                return Err(e);
            }
        };

        let (locals, upvalues, _captured0, disposables0) = scope.pop_frame();
        for (target, is_await) in disposables0.into_iter().rev() {
            body.push(HirStmt::Dispose { target, is_await });
        }
        let func = HirFunction {
            name,
            params,
            locals,
            body,
            return_ty: HirType::Dynamic,
            upvalue_count: upvalues.len() as u32,
            has_this,
            has_rest: params_ast.iter().any(|p| p.is_rest),
            is_async,
            is_generator,
        };
        Ok((func, upvalues))
    }

    fn lower_function_body(
        &mut self,
        params_ast: &[Param],
        body: BodyRef<'_>,
        field_inits: &[(Rc<str>, &Expr)],
        scope: &mut Scope,
    ) -> R<(Vec<HirParam>, Vec<HirStmt>)> {
        let mut params = Vec::new();
        let mut destructuring_params = Vec::new();
        for (i, p) in params_ast.iter().enumerate() {
            let pname = match &p.pattern {
                Pattern::Identifier { name, .. } => name.clone(),
                _ => {
                    let name = Rc::from(format!("__p{}", i + 1));
                    destructuring_params.push((i, &p.pattern));
                    name
                }
            };
            scope.define(pname.clone(), HirBinding::Param(i as u32));
            let ty = self.value_ty(AnnKey::decl(p.range.start.offset));
            params.push(HirParam {
                name: pname,
                ty,
                default: None,
            });
        }

        for (i, p) in params_ast.iter().enumerate() {
            if let Some(default) = &p.default {
                params[i].default = Some(self.lower_expr(default, scope)?);
            }
        }
        let mut out = Vec::new();
        for (i, pat) in destructuring_params {
            self.desugar_pattern_local(
                pat,
                HirExpr::Var(HirBinding::Param(i as u32)),
                scope,
                &mut out,
            )?;
        }
        for (fname, fexpr) in field_inits {
            let value = self.lower_expr(fexpr, scope)?;
            out.push(HirStmt::SetMember {
                object: HirExpr::This,
                name: fname.clone(),
                value,
            });
        }

        match body {
            BodyRef::Block(stmt) => match &stmt.kind {
                StmtKind::Block { stmts } => {
                    for s in stmts {
                        self.lower_stmt(s, scope, &mut out)?;
                    }
                }
                _ => self.lower_stmt(stmt, scope, &mut out)?,
            },
            BodyRef::ExprBody(e) => {
                let v = self.lower_expr(e, scope)?;
                out.push(HirStmt::Return(Some(v)));
            }
            BodyRef::Empty => {}
        }
        Ok((params, out))
    }
}
