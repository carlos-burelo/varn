use super::type_inference::widen_literal;
use super::Binder;
use crate::types::{FunctionParam, Type};
use std::sync::Arc;
use varn_core::ast::{Param, VarDeclarator};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParamSite {
    Declared,
    Closure,
}

impl<'r> Binder<'r> {
    pub(crate) fn param_type(&mut self, p: &Param, site: ParamSite) -> Type {
        let ty = match (&p.type_ann, p.default) {
            (Some(ann), _) => self.resolve_type(ann),
            (None, Some(default)) => widen_literal(self.infer_expr_type_self(default)),
            (None, None) => match site {
                ParamSite::Declared => {
                    self.report_missing_param_annotation(p);
                    Type::Error
                }
                ParamSite::Closure => Type::Dynamic,
            },
        };
        if p.is_rest && !matches!(self.ty_table.get(ty.0), varn_core::TypeKind::Array(_)) {
            Type::array(ty, &mut *Arc::make_mut(&mut self.ty_table))
        } else {
            ty
        }
    }

    pub(crate) fn function_param(&mut self, p: &Param, site: ParamSite) -> FunctionParam {
        let ty = self.param_type(p, site);
        FunctionParam {
            name: Some(Arc::from(super::pattern_lead_name(
                &p.pattern,
                &self.interner,
            ))),
            ty: ty.0,
            optional: p.is_optional || p.default.is_some(),
            is_rest: p.is_rest,
        }
    }

    fn report_missing_param_annotation(&mut self, p: &Param) {
        if !self.reported_params.insert(p.range.start.offset) {
            return;
        }
        let name = super::pattern_lead_name(&p.pattern, &self.interner).to_owned();
        self.emit(
            varn_core::Diagnostic::error(
                varn_core::ErrorCode::TypeAnnotationRequired,
                format!("parameter '{name}' needs a type annotation or a default value"),
            )
            .with_range(p.range),
        );
    }

    pub(crate) fn report_missing_variable_annotation(&mut self, d: &VarDeclarator) {
        let name = super::pattern_lead_name(&d.id, &self.interner).to_owned();
        self.emit(
            varn_core::Diagnostic::error(
                varn_core::ErrorCode::TypeAnnotationRequired,
                format!("variable '{name}' needs a type annotation or an initializer"),
            )
            .with_range(d.range),
        );
    }
}
