use std::sync::Arc;
use varn_tir::*;

pub fn module_with_point() -> TirModule {
    let mut types = TyTable::default();
    let _ = types.intern(BackendTy::Int);
    TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![ClassInfo::new(
            Arc::from("Point"),
            varn_tir::Ancestry::Root,
            vec![
                ("x".into(), BackendTy::Int),
                ("label".into(), BackendTy::Str),
            ],
            &varn_tir::TyTable::default(),
        )],
        enums: vec![],
        signatures: vec![Signature {
            params: vec![],
            return_ty: BackendTy::Void,
            has_rest: false,
        }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        class_defs: vec![],
        top_level: TirFunction {
            force_inline: false,
            name: Arc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
        },
    }
}

pub fn expr(kind: TirExprKind, ty: BackendTy, res: Resolution) -> TirExpr {
    TirExpr {
        kind,
        ty,
        res,
        span: Span::EMPTY,
    }
}

pub fn int(v: i64) -> TirExpr {
    expr(TirExprKind::IntLit(v), BackendTy::Int, Resolution::None)
}
