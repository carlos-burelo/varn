use std::rc::Rc;

use crate::hir::{HirClass, HirEnum, HirExpr, HirFunction, HirType, HirUpvalueSrc};
use crate::ssa::ir::{InstKind, Value};

use super::{Builder, Result};

impl Builder {
    pub(super) fn lower_class(&mut self, cls: &HirClass) -> Result<Value> {
        let name_idx = cls.name.clone();
        let super_class = match &cls.super_class {
            Some(sup) => Some(self.lower_expr(sup)?),
            None => None,
        };
        let mut class_val = self.emit(
            InstKind::MakeClass {
                name: name_idx,
                super_class,
            },
            HirType::Ref,
        );

        for field in &cls.fields {
            self.emit_effect(InstKind::DeclareField {
                class: class_val,
                name: field.clone(),
            });
        }

        for (key, init) in &cls.static_fields {
            let val = match init {
                Some(e) => self.lower_expr(e)?,
                None => self.emit(InstKind::ConstNull, HirType::Ref),
            };
            self.emit_effect(InstKind::DefineStatic {
                class: class_val,
                name: key.clone(),
                value: val,
            });
        }

        self.bind_method(class_val, &cls.ctor, false)?;
        for m in &cls.methods {
            self.bind_method(class_val, m, false)?;
        }
        for m in &cls.static_methods {
            self.bind_method(class_val, m, true)?;
        }
        for a in &cls.getters {
            self.bind_member(
                class_val,
                a.key.clone(),
                &a.func,
                &a.upvalues,
                true,
                a.is_static,
            )?;
        }
        for a in &cls.setters {
            self.bind_member(
                class_val,
                a.key.clone(),
                &a.func,
                &a.upvalues,
                false,
                a.is_static,
            )?;
        }

        if !cls.static_blocks.is_empty() {
            let qualified_name = if let Some(ref src) = self.source_file {
                Rc::from(format!("{}::{}", src.replace('\\', "/"), cls.name))
            } else {
                cls.name.clone()
            };
            self.emit_effect(InstKind::StoreGlobal {
                name: qualified_name,
                value: class_val,
            });
        }

        for b in &cls.static_blocks {
            let fn_val = self.lower_closure(&b.func, &b.upvalues)?;
            self.emit(
                InstKind::Call {
                    callee: fn_val,
                    args: Vec::new(),
                },
                HirType::Dynamic,
            );
        }

        if !cls.decorators.is_empty() {
            class_val = self.apply_class_decorators(class_val, &cls.decorators)?;
        }

        Ok(class_val)
    }

    pub(super) fn lower_enum(&mut self, en: &HirEnum) -> Result<Value> {
        let name_idx = en.name.clone();
        let class_val = self.emit(
            InstKind::MakeClass {
                name: name_idx,
                super_class: None,
            },
            HirType::Ref,
        );

        for v in &en.variants {
            let variant_val = self.emit(
                InstKind::MakeEnumVariant {
                    tag: v.tag,
                    meta: v.meta.clone(),
                },
                HirType::Ref,
            );
            self.emit_effect(InstKind::DefineStatic {
                class: class_val,
                name: v.name.clone(),
                value: variant_val,
            });
        }

        for field in &en.fields {
            self.emit_effect(InstKind::DeclareField {
                class: class_val,
                name: field.clone(),
            });
        }

        for (key, init) in &en.static_fields {
            let val = match init {
                Some(e) => self.lower_expr(e)?,
                None => self.emit(InstKind::ConstNull, HirType::Ref),
            };
            self.emit_effect(InstKind::DefineStatic {
                class: class_val,
                name: key.clone(),
                value: val,
            });
        }

        self.bind_method(class_val, &en.ctor, false)?;
        for m in &en.methods {
            self.bind_method(class_val, m, false)?;
        }
        for m in &en.static_methods {
            self.bind_method(class_val, m, true)?;
        }
        for a in &en.getters {
            self.bind_member(
                class_val,
                a.key.clone(),
                &a.func,
                &a.upvalues,
                true,
                a.is_static,
            )?;
        }
        for a in &en.setters {
            self.bind_member(
                class_val,
                a.key.clone(),
                &a.func,
                &a.upvalues,
                false,
                a.is_static,
            )?;
        }

        for v in &en.variants {
            if !v.const_args.is_empty() {
                let receiver = self.emit(
                    InstKind::GetProperty {
                        object: class_val,
                        name: v.name.clone(),
                    },
                    HirType::Ref,
                );
                let mut args = Vec::with_capacity(v.const_args.len());
                for arg in &v.const_args {
                    args.push(self.lower_expr(arg)?);
                }
                self.emit(
                    InstKind::MethodCall {
                        recv: receiver,
                        name: Rc::from("constructor"),
                        args,
                    },
                    HirType::Dynamic,
                );
            }
        }

        if !en.static_blocks.is_empty() {
            let qualified_name = if let Some(ref src) = self.source_file {
                Rc::from(format!("{}::{}", src.replace('\\', "/"), en.name))
            } else {
                en.name.clone()
            };
            self.emit_effect(InstKind::StoreGlobal {
                name: qualified_name,
                value: class_val,
            });
        }

        for b in &en.static_blocks {
            let fn_val = self.lower_closure(&b.func, &b.upvalues)?;
            self.emit(
                InstKind::Call {
                    callee: fn_val,
                    args: Vec::new(),
                },
                HirType::Dynamic,
            );
        }

        Ok(class_val)
    }

    fn bind_method(
        &mut self,
        class_val: Value,
        m: &crate::hir::HirMethod,
        is_static: bool,
    ) -> Result<()> {
        let mut reg = self.lower_closure(&m.func, &m.upvalues)?;
        if !m.decorators.is_empty() {
            reg =
                self.apply_method_decorators(reg, &m.key, is_static, m.is_private, &m.decorators)?;
        }
        self.emit_effect(InstKind::DefineMethod {
            class: class_val,
            name: m.key.clone(),
            method: reg,
            is_static,
        });
        Ok(())
    }

    fn bind_member(
        &mut self,
        class_val: Value,
        name: Rc<str>,
        func: &HirFunction,
        upvalues: &[HirUpvalueSrc],
        is_getter: bool,
        is_static: bool,
    ) -> Result<()> {
        let reg = self.lower_closure(func, upvalues)?;
        self.emit_effect(InstKind::DefineAccessor {
            class: class_val,
            name,
            accessor: reg,
            is_getter,
            is_static,
        });
        Ok(())
    }

    fn apply_method_decorators(
        &mut self,
        method_val: Value,
        key: &str,
        is_static: bool,
        is_private: bool,
        decorators: &[HirExpr],
    ) -> Result<Value> {
        let mut current_method = method_val;
        for deco in decorators.iter().rev() {
            let deco_fn = self.lower_expr(deco)?;
            let n_r = self.emit(InstKind::ConstStr(Rc::from(key)), HirType::Str);
            let kind_r = self.emit(InstKind::ConstStr(Rc::from("method")), HirType::Str);
            let static_r = self.emit(InstKind::ConstBool(is_static), HirType::Bool);
            let private_r = self.emit(InstKind::ConstBool(is_private), HirType::Bool);

            let pairs = vec![
                (Rc::from("name"), n_r),
                (Rc::from("kind"), kind_r),
                (Rc::from("isStatic"), static_r),
                (Rc::from("isPrivate"), private_r),
            ];
            let ctx_obj = self.emit(InstKind::BuildObject { pairs }, HirType::Ref);
            let args = vec![current_method, ctx_obj];
            let result = self.emit(
                InstKind::Call {
                    callee: deco_fn,
                    args,
                },
                HirType::Dynamic,
            );
            let isnull = self.emit(InstKind::IsNull { operand: result }, HirType::Bool);
            current_method = self.lower_branch_value(
                isnull,
                |_| Ok(current_method),
                |_| Ok(result),
                HirType::Ref,
            )?;
        }
        Ok(current_method)
    }

    fn apply_class_decorators(
        &mut self,
        class_val: Value,
        decorators: &[HirExpr],
    ) -> Result<Value> {
        let mut current_class = class_val;
        for deco in decorators.iter().rev() {
            let deco_fn = self.lower_expr(deco)?;
            let args = vec![current_class];
            let result = self.emit(
                InstKind::Call {
                    callee: deco_fn,
                    args,
                },
                HirType::Dynamic,
            );
            let isnull = self.emit(InstKind::IsNull { operand: result }, HirType::Bool);
            current_class = self.lower_branch_value(
                isnull,
                |_| Ok(current_class),
                |_| Ok(result),
                HirType::Ref,
            )?;
        }
        Ok(current_class)
    }
}
