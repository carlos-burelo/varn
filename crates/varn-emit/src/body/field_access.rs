use super::context::FnEmitter;
use std::sync::Arc;
use varn_tir::{BackendTy, Resolution, Span, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn field_access(
        &mut self,
        obj: TirExpr,
        name: Arc<str>,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        self.member_access(obj, name, ty, span, true)
    }

    pub(super) fn field_place(
        &mut self,
        obj: TirExpr,
        name: Arc<str>,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        self.member_access(obj, name, ty, span, false)
    }

    fn member_access(
        &mut self,
        obj: TirExpr,
        name: Arc<str>,
        ty: BackendTy,
        span: Span,
        read: bool,
    ) -> TirExpr {
        let recv_ty = obj.ty;
        if read && let BackendTy::Enum(id) = obj.ty.non_nullable(self.tt) {
            let getter = varn_core::AccessorKind::Get.key(&name);
            let slot = self
                .m
                .enums
                .get(id.0 as usize)
                .and_then(|info| info.member_slot(&getter));
            if let Some(slot) = slot {
                return TirExpr {
                    kind: TirExprKind::MethodCall {
                        recv: Box::new(obj),
                        name: Arc::from(getter),
                        args: Vec::new(),
                    },
                    ty,
                    res: Resolution::VtableSlot(slot),
                    span,
                };
            }
        }
        if read
            && name.as_ref() == varn_core::MemberKey::Length.as_str()
            && matches!(
                obj.ty.non_nullable(self.tt),
                BackendTy::Array(_) | BackendTy::Tuple(_) | BackendTy::Str | BackendTy::Bytes
            )
        {
            return TirExpr {
                kind: TirExprKind::Length {
                    operand: Box::new(obj),
                },
                ty: BackendTy::Int,
                res: Resolution::None,
                span,
            };
        }
        if read
            && obj.ty == obj.ty.non_nullable(self.tt)
            && let Some(cls) = self.core_class_name(obj.ty)
            && let Some(&op_id) = self.m.core_ops.get(&(
                Arc::from(cls),
                Arc::from(varn_core::AccessorKind::Get.key(&name)),
            ))
        {
            return TirExpr {
                kind: TirExprKind::MethodCall {
                    recv: Box::new(obj),
                    name,
                    args: Vec::new(),
                },
                ty,
                res: Resolution::NativeOp(op_id),
                span,
            };
        }
        if let BackendTy::Enum(id) = obj.ty.non_nullable(self.tt)
            && let Some(info) = self.m.enums.get(id.0 as usize)
            && Self::is_class_ref(
                &obj,
                info.binding.as_ref().unwrap_or(&info.name),
                self.m.globals,
            )
            && let Some(variant) = info.variants.iter().find(|v| v.name == name)
            && info
                .variant_global(&variant.name)
                .is_some_and(|global| self.m.globals.contains_key(&global))
        {
            return TirExpr {
                kind: TirExprKind::Field {
                    object: Box::new(obj),
                    name,
                },
                ty,
                res: Resolution::EnumVariant {
                    enum_id: id,
                    tag: variant.tag,
                },
                span,
            };
        }
        if let Some(ci) = self.class_of(obj.ty) {
            if Self::is_class_ref(&obj, &ci.name, self.m.globals) {
                if let Some(slot) = ci.static_slot(&name) {
                    let declared = ci.static_at(slot).map(|s| s.ty).unwrap_or(ty);
                    let ty = if ty != declared && ty == declared.non_nullable(self.tt) {
                        ty
                    } else {
                        declared
                    };
                    return TirExpr {
                        kind: TirExprKind::Field {
                            object: Box::new(obj),
                            name,
                        },
                        ty,
                        res: Resolution::StaticField(slot),
                        span,
                    };
                }
                return TirExpr {
                    kind: TirExprKind::Field {
                        object: Box::new(obj),
                        name: name.clone(),
                    },
                    ty,
                    res: self.by_name(name, recv_ty),
                    span,
                };
            }
            if let Some(field) = ci.field(&name).cloned() {
                let keeps_checked = ty == field.ty
                    || ty == field.ty.non_nullable(self.tt)
                    || (matches!(field.ty, BackendTy::Dynamic(_))
                        && !matches!(ty, BackendTy::Dynamic(_)));
                let ty = if keeps_checked { ty } else { field.ty };
                return TirExpr {
                    kind: TirExprKind::Field {
                        object: Box::new(obj),
                        name,
                    },
                    ty,
                    res: Resolution::FieldSlot(field.slot),
                    span,
                };
            }
            let getter = varn_core::AccessorKind::Get.key(&name);
            if read && let Some(slot) = ci.method_slot(&getter) {
                return TirExpr {
                    kind: TirExprKind::MethodCall {
                        recv: Box::new(obj),
                        name: Arc::from(getter),
                        args: Vec::new(),
                    },
                    ty,
                    res: Resolution::VtableSlot(slot),
                    span,
                };
            }
        }
        TirExpr {
            kind: TirExprKind::Field {
                object: Box::new(obj),
                name: name.clone(),
            },
            ty,
            res: self.by_name(name, recv_ty),
            span,
        }
    }
}
