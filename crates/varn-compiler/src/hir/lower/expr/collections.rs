use std::rc::Rc;

use varn_core::ast::expr::{ArrayEl, ObjectProp, PropKey};
use varn_core::ast::{Expr, ExprKind};

use super::*;

impl<'a> Lowerer<'a> {
    /// Array, tuple, record and object literals.
    pub(super) fn lower_collection_expr(&mut self, expr: &Expr, scope: &mut Scope) -> R<HirExpr> {
        match &expr.kind {
            ExprKind::Array { elements } => {
                let mut out = Vec::with_capacity(elements.len());
                for el in elements {
                    match el {
                        ArrayEl::Expr(e) => out.push(HirArrayEl::Expr(self.lower_expr(e, scope)?)),
                        ArrayEl::Spread(e) => {
                            out.push(HirArrayEl::Spread(self.lower_expr(e, scope)?))
                        }
                        ArrayEl::Hole => out.push(HirArrayEl::Hole),
                    }
                }
                Ok(HirExpr::Array(out))
            }
            ExprKind::Tuple { elements } => {
                let mut out = Vec::with_capacity(elements.len());
                for e in elements {
                    out.push(HirArrayEl::Expr(self.lower_expr(e, scope)?));
                }
                Ok(HirExpr::Tuple(out))
            }
            ExprKind::Record { properties } => {
                let mut props = Vec::with_capacity(properties.len());
                for prop in properties {
                    if let ObjectProp::Property { key, value, .. } = prop {
                        let k = match key {
                            PropKey::Computed(e) => {
                                HirPropKey::Computed(self.lower_expr(e, scope)?)
                            }
                            PropKey::Identifier(s) | PropKey::Str(s) => {
                                HirPropKey::Static(Rc::from(s.as_str()))
                            }
                            PropKey::Int(n) => HirPropKey::Static(Rc::from(n.to_string().as_str())),
                        };
                        let val = self.lower_expr(value, scope)?;
                        props.push(HirObjectProp::Property { key: k, value: val });
                    }
                }
                Ok(HirExpr::Record { properties: props })
            }
            ExprKind::Object { properties } => {
                let mut props = Vec::with_capacity(properties.len());
                for prop in properties {
                    match prop {
                        ObjectProp::Property { key, value, .. } => {
                            let k = match key {
                                PropKey::Computed(e) => {
                                    HirPropKey::Computed(self.lower_expr(e, scope)?)
                                }
                                PropKey::Identifier(s) | PropKey::Str(s) => {
                                    HirPropKey::Static(Rc::from(s.as_str()))
                                }
                                PropKey::Int(n) => {
                                    HirPropKey::Static(Rc::from(n.to_string().as_str()))
                                }
                            };
                            let val = self.lower_expr(value, scope)?;
                            props.push(HirObjectProp::Property { key: k, value: val });
                        }
                        ObjectProp::Method {
                            key,
                            params,
                            body,
                            is_async,
                            is_generator,
                            ..
                        } => {
                            let k = match key {
                                PropKey::Computed(e) => {
                                    HirPropKey::Computed(self.lower_expr(e, scope)?)
                                }
                                PropKey::Identifier(s) | PropKey::Str(s) => {
                                    HirPropKey::Static(Rc::from(s.as_str()))
                                }
                                PropKey::Int(n) => {
                                    HirPropKey::Static(Rc::from(n.to_string().as_str()))
                                }
                            };
                            let method_name = match key {
                                PropKey::Identifier(s) | PropKey::Str(s) => s.clone(),
                                PropKey::Int(n) => n.to_string(),
                                PropKey::Computed(_) => "<computed>".to_owned(),
                            };
                            let return_ty = self.value_ty(AnnKey::decl(body.range.start.offset));
                            let (func, upvalues) = self.lower_function_like(
                                Rc::from(method_name.as_str()),
                                params,
                                *is_async,
                                *is_generator,
                                false,
                                true,
                                body.range.start.line,
                                BodyRef::Block(body),
                                &[],
                                return_ty,
                                scope,
                            )?;
                            props.push(HirObjectProp::Method {
                                key: k,
                                func,
                                upvalues,
                            });
                        }
                        ObjectProp::Spread { argument, .. } => {
                            let arg = self.lower_expr(argument, scope)?;
                            props.push(HirObjectProp::Spread(arg));
                        }
                        _ => {}
                    }
                }
                Ok(HirExpr::Object { properties: props })
            }
            ExprKind::With { object, properties } => {
                let mut props = Vec::with_capacity(1 + properties.len());
                let base = self.lower_expr(object, scope)?;
                props.push(HirObjectProp::Spread(base));
                for prop in properties {
                    match prop {
                        ObjectProp::Property { key, value, .. } => {
                            let k = match key {
                                PropKey::Computed(e) => {
                                    HirPropKey::Computed(self.lower_expr(e, scope)?)
                                }
                                PropKey::Identifier(s) | PropKey::Str(s) => {
                                    HirPropKey::Static(Rc::from(s.as_str()))
                                }
                                PropKey::Int(n) => {
                                    HirPropKey::Static(Rc::from(n.to_string().as_str()))
                                }
                            };
                            let val = self.lower_expr(value, scope)?;
                            props.push(HirObjectProp::Property { key: k, value: val });
                        }
                        ObjectProp::Spread { argument, .. } => {
                            let arg = self.lower_expr(argument, scope)?;
                            props.push(HirObjectProp::Spread(arg));
                        }
                        _ => {}
                    }
                }
                Ok(HirExpr::Object { properties: props })
            }
            other => unreachable!("lower_collection_expr: {other:?} is not handled here"),
        }
    }
}
