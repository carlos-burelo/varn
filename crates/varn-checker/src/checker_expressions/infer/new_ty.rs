use super::Checker;
use crate::binder::BindResult;
use crate::types::Type;
use varn_core::ast::ExprId;
use varn_core::TypeKind;

impl<'r> Checker<'r> {
    pub(super) fn infer_new_type(
        &mut self,
        callee: ExprId,
        type_args: &[varn_core::ast::TypeNode],
        bind: &BindResult,
    ) -> Type {
        let callee_ty = self.infer_type(callee, bind);
        if callee_ty.is_dynamic() {
            return Type::Dynamic;
        }
        let callee_kind = self.ty_table.get(callee_ty.0);
        match callee_kind {
            TypeKind::Named(name, origin) => {
                let name_str = bind.interner.resolve(name).to_string();
                let origin_str = origin.map(|o| std::sync::Arc::from(bind.interner.resolve(o)));
                if !type_args.is_empty() {
                    let args: Vec<Type> = type_args
                        .iter()
                        .map(|a| self.resolve_type_node_cached(a, bind))
                        .collect();
                    Type::generic_with_origin(
                        name_str,
                        args,
                        origin_str,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                } else if name_str == varn_core::BuiltinType::Map.name() {
                    Type::generic_with_origin(
                        name_str,
                        vec![Type::Dynamic, Type::Dynamic],
                        origin_str,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                } else {
                    Type::named_with_origin(
                        name_str,
                        origin_str,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                }
            }
            TypeKind::Generic(name, args, origin) => {
                let name_str = bind.interner.resolve(name).to_string();
                let origin_str = origin.map(|o| std::sync::Arc::from(bind.interner.resolve(o)));
                let arg_ids = self.ty_table.get_list(args).to_vec();
                let args: Vec<Type> = arg_ids.into_iter().map(|id| Type(id, false)).collect();
                Type::generic_with_origin(
                    name_str,
                    args,
                    origin_str,
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                )
            }
            _ => {
                if let varn_core::ast::ExprKind::Identifier { name } =
                    &self.ast_arena.expr(callee).kind
                {
                    let name_str = bind.interner.resolve(*name).to_string();
                    if !type_args.is_empty() {
                        let args: Vec<Type> = type_args
                            .iter()
                            .map(|a| self.resolve_type_node_cached(a, bind))
                            .collect();
                        Type::generic(
                            name_str,
                            args,
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        )
                    } else if name_str == varn_core::BuiltinType::Map.name() {
                        Type::generic(
                            name_str,
                            vec![Type::Dynamic, Type::Dynamic],
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        )
                    } else {
                        Type::named(name_str, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
                    }
                } else {
                    Type::Dynamic
                }
            }
        }
    }
}
