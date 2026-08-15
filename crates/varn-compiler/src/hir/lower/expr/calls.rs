use std::rc::Rc;
use varn_core::AnnKey;

use varn_core::ast::{Arg, Expr, ExprKind};

use super::*;

impl<'a> Lowerer<'a> {
    /// Calls and member access, including the pipeline operator and tagged
    /// templates, which both desugar into calls.
    pub(super) fn lower_call_expr(&mut self, expr: &Expr, scope: &mut Scope) -> R<HirExpr> {
        let offset = expr.range.start.offset;
        let key = AnnKey::expr(expr.id);
        match &expr.kind {
            ExprKind::Call {
                callee,
                args,
                optional,
                ..
            } => {
                if *optional {
                    let hargs = self.lower_call_args(args, key, scope)?;
                    let callee_hir = self.lower_expr(callee, scope)?;
                    return Ok(HirExpr::OptionalChain {
                        object: Box::new(callee_hir),
                        property: HirOptionalProperty::Call(hargs),
                    });
                }
                // Free-function intrinsic (`abs(x)` from `std:math`): lower to
                // IntrinsicCall with a synthetic null receiver so it reuses the
                // method-form codegen (VM reads args[1]; JIT inlines
                // fabs/sqrtsd/roundsd). Keyed at the callee identifier offset.
                if let ExprKind::Identifier { .. } = &callee.kind {
                    if let Some(wire_byte) = self.ann.get_intrinsic(AnnKey::expr(callee.id)) {
                        let has_spread = args.iter().any(|a| matches!(a, Arg::Spread(_)));
                        if !has_spread {
                            let hargs = self.lower_call_args(args, key, scope)?;
                            // The checker recorded the result type at the
                            // expression start for a plain call. Dropping it
                            // (this was `Dynamic`) made the result register
                            // share a slot with differently-typed values, so
                            // `derive_register_meta` met the two to `Dynamic`
                            // and every float op consuming an intrinsic fell
                            // off the native f64 path onto the generic helper.
                            let ty = self.value_ty(AnnKey::expr(expr.id));
                            return Ok(HirExpr::IntrinsicCall {
                                object: Box::new(HirExpr::Null),
                                args: hargs,
                                wire_byte,
                                ty,
                            });
                        }
                    }
                }

                // Intrinsic / native-op annotations are keyed by the METHOD
                // NAME offset: chained calls (`a.f().g()`) share their
                // expression start offset, so keying by expr start let an
                // outer call's annotation leak into the inner call.
                let method_key = match &callee.kind {
                    ExprKind::Member {
                        property,
                        computed: false,
                        ..
                    } => Some(AnnKey::expr(property.id)),
                    _ => None,
                };
                if let Some(wire_byte) = method_key.and_then(|o| self.ann.get_intrinsic(o)) {
                    if let ExprKind::Member {
                        object,
                        computed: false,
                        ..
                    } = &callee.kind
                    {
                        let has_spread = args.iter().any(|a| matches!(a, Arg::Spread(_)));
                        if !has_spread {
                            let hobj = self.lower_expr(object, scope)?;
                            let hargs = self.lower_call_args(args, key, scope)?;
                            // Method form keys at the method-name offset (a
                            // chain shares its expression start). Same reason
                            // as the free-function arm above.
                            let ty = self.value_ty(method_key.unwrap_or(key));
                            return Ok(HirExpr::IntrinsicCall {
                                object: Box::new(hobj),
                                args: hargs,
                                wire_byte,
                                ty,
                            });
                        }
                    }
                }
                if let Some(op_id) = method_key.and_then(|o| self.ann.get_native_op(o)) {
                    if let ExprKind::Member {
                        object,
                        computed: false,
                        ..
                    } = &callee.kind
                    {
                        let has_spread = args.iter().any(|a| matches!(a, Arg::Spread(_)));
                        if !has_spread {
                            let hobj = self.lower_expr(object, scope)?;
                            let hargs = self.lower_call_args(args, key, scope)?;
                            return Ok(HirExpr::NativeMethodCall {
                                object: Box::new(hobj),
                                args: hargs,
                                op_id,
                                ty: HirType::Dynamic,
                            });
                        }
                    }
                }

                if let Some(mangled) = self.extension_calls.get(&offset).cloned() {
                    if let ExprKind::Member {
                        object,
                        optional: mem_opt,
                        ..
                    } = &callee.kind
                    {
                        let recv = self.lower_expr(object, scope)?;
                        let call_args = self.lower_call_args(args, key, scope)?;
                        if *mem_opt {
                            return Ok(HirExpr::OptionalChain {
                                object: Box::new(recv),
                                property: HirOptionalProperty::ExtensionCall(mangled, call_args),
                            });
                        }
                        return Ok(self.ext_global_call(mangled, recv, call_args));
                    }
                }

                let hargs = self.lower_call_args(args, key, scope)?;

                if matches!(callee.kind, ExprKind::Super) {
                    return Ok(HirExpr::SuperCall { args: hargs });
                }

                if let ExprKind::Member {
                    object,
                    property,
                    computed: false,
                    optional: false,
                } = &callee.kind
                {
                    if matches!(object.kind, ExprKind::Super) {
                        if let ExprKind::Identifier { name } = &property.kind {
                            return Ok(HirExpr::SuperMethodCall {
                                name: name.clone(),
                                args: hargs,
                            });
                        }
                    }
                }

                if let ExprKind::Member {
                    object,
                    property,
                    computed: false,
                    optional: mem_opt,
                } = &callee.kind
                {
                    if !matches!(object.kind, ExprKind::Super) {
                        if let ExprKind::Identifier { name } = &property.kind {
                            let recv = Box::new(self.lower_expr(object, scope)?);
                            if *mem_opt {
                                return Ok(HirExpr::OptionalChain {
                                    object: recv,
                                    property: HirOptionalProperty::MethodCall(name.clone(), hargs),
                                });
                            }
                            let ty = self.value_ty(AnnKey::expr(property.id));
                            return Ok(HirExpr::MethodCall {
                                recv,
                                name: name.clone(),
                                args: hargs,
                                ty,
                            });
                        }
                    }
                }

                let has_spread = args.iter().any(|a| matches!(a, Arg::Spread(_)));
                if !has_spread && self.is_self_call(callee, scope) {
                    return Ok(HirExpr::SelfCall {
                        args: hargs,
                        ty: HirType::Dynamic,
                    });
                }
                let call_ty = self.value_ty(key);
                let callee = Box::new(self.lower_expr(callee, scope)?);
                Ok(HirExpr::Call {
                    callee,
                    args: hargs,
                    ty: call_ty,
                })
            }
            ExprKind::Member {
                object,
                property,
                computed,
                optional,
            } => {
                if *optional {
                    let object_hir = self.lower_expr(object, scope)?;
                    let opt_prop = if *computed {
                        let idx = self.lower_expr(property, scope)?;
                        HirOptionalProperty::Index(Box::new(idx))
                    } else {
                        let name = match &property.kind {
                            ExprKind::Identifier { name } => name.clone(),
                            _ => return Err(OptError::Unsupported("hir: non-identifier property")),
                        };
                        // Module slots are recorded on export/import DECLARATIONS, so
                        // the lookup stays in that space — the same byte-offset
                        // match it has always made, now said out loud.
                        if self.ann.get_slot_idx(AnnKey::decl(offset)).is_some() {
                            let slot_idx =
                                self.ann.get_slot_idx(AnnKey::decl(offset)).unwrap() as u16;
                            HirOptionalProperty::ModuleSlot(slot_idx)
                        } else if let Some(mangled) = self
                            .extension_members
                            .get(&property.range.start.offset)
                            .or_else(|| self.extension_members.get(&offset))
                            .cloned()
                        {
                            HirOptionalProperty::Extension(mangled)
                        } else {
                            HirOptionalProperty::Member(name)
                        }
                    };
                    return Ok(HirExpr::OptionalChain {
                        object: Box::new(object_hir),
                        property: opt_prop,
                    });
                }
                if *computed {
                    let ty = self.value_ty(AnnKey::expr(property.id));
                    let object = Box::new(self.lower_expr(object, scope)?);
                    let index = Box::new(self.lower_expr(property, scope)?);
                    let is_array = self.ann.get_array_index(key);
                    return Ok(HirExpr::Index {
                        object,
                        index,
                        ty,
                        is_array,
                    });
                }

                if let Some(slot_idx) = self.ann.get_slot_idx(AnnKey::decl(offset)) {
                    let object_hir = self.lower_expr(object, scope)?;
                    return Ok(HirExpr::ModuleSlot {
                        object: Box::new(object_hir),
                        slot: slot_idx as u16,
                        ty: HirType::Dynamic,
                    });
                }

                if let Some(slot) = self.ann.get_fixed_field_slot(AnnKey::expr(property.id)) {
                    let ty = self.value_ty(AnnKey::expr(property.id));
                    let object_hir = self.lower_expr(object, scope)?;
                    return Ok(HirExpr::GetFixedField {
                        object: Box::new(object_hir),
                        slot,
                        ty,
                    });
                }

                if let Some(mangled) = self
                    .extension_members
                    .get(&property.range.start.offset)
                    .or_else(|| self.extension_members.get(&offset))
                    .cloned()
                {
                    let recv = self.lower_expr(object, scope)?;
                    return Ok(self.ext_global_call(mangled, recv, vec![]));
                }
                if matches!(object.kind, ExprKind::Super) {
                    let name = match &property.kind {
                        ExprKind::Identifier { name } => name.clone(),
                        _ => {
                            return Err(OptError::Unsupported("hir: non-identifier super property"))
                        }
                    };
                    return Ok(HirExpr::SuperMember { name });
                }
                let name = match &property.kind {
                    ExprKind::Identifier { name } => name.clone(),
                    _ => return Err(OptError::Unsupported("hir: non-identifier property")),
                };
                let ty = self.value_ty(AnnKey::expr(property.id));
                let object = Box::new(self.lower_expr(object, scope)?);
                Ok(HirExpr::Member { object, name, ty })
            }
            ExprKind::Pipeline { left, right } => {
                if pipeline_has_placeholder(right) {
                    let range = varn_core::SourceRange::default();
                    let param = varn_core::ast::Param {
                        pattern: varn_core::ast::Pattern::Identifier {
                            name: Rc::from("_"),
                            type_ann: None,
                            range,
                        },
                        type_ann: None,
                        default: None,
                        is_rest: false,
                        is_optional: false,
                        modifiers: varn_core::ast::operators::Modifiers::default(),
                        range,
                    };
                    let body_expr = right.clone();
                    let body_ref = BodyRef::ExprBody(&body_expr);
                    let (func, upvalues) = self.lower_function_like(
                        Rc::from("<pipe>"),
                        &[param],
                        false,
                        false,
                        false,
                        false,
                        body_ref,
                        &[],
                        scope,
                    )?;
                    let callee = HirExpr::Closure {
                        func: Box::new(func),
                        upvalues,
                    };
                    let arg = self.lower_expr(left, scope)?;
                    Ok(HirExpr::Call {
                        callee: Box::new(callee),
                        args: vec![arg],
                        ty: HirType::Dynamic,
                    })
                } else {
                    let callee = Box::new(self.lower_expr(right, scope)?);
                    let arg = self.lower_expr(left, scope)?;
                    Ok(HirExpr::Call {
                        callee,
                        args: vec![arg],
                        ty: HirType::Dynamic,
                    })
                }
            }
            ExprKind::TaggedTemplate { tag, template, .. } => {
                let htag = self.lower_expr(tag, scope)?;
                let htpl = self.lower_expr(template, scope)?;
                Ok(HirExpr::TaggedTemplate {
                    tag: Box::new(htag),
                    template: Box::new(htpl),
                })
            }
            other => unreachable!("lower_call_expr: {other:?} is not handled here"),
        }
    }
}
