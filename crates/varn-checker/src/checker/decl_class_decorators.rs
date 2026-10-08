use super::recorder::Recorder;
use super::Checker;
use varn_core::ast::ClassMember;
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn check_class_decorators(
        &mut self,
        rec: &mut Recorder,
        c: &varn_core::ast::ClassDecl,
        bind: &BindResult,
    ) {
        if !c.decorators.is_empty() {
            let name =
                c.id.map(|id| bind.interner.resolve(id))
                    .unwrap_or("<anonymous>");
            self.check_decorator_signatures(
                rec,
                &c.decorators,
                super::decorator_signature::DecoratorTarget::Class,
                name,
                bind,
            );
        }
        for member in &c.body {
            match member {
                ClassMember::Method {
                    key,
                    body: Some(body),
                    params,
                    modifiers,
                    range,
                    ..
                } if modifiers.is_static
                    && super::decorator_receiver::method_uses_receiver(
                        self.ast_arena,
                        params,
                        *body,
                    ) =>
                {
                    let key_str = bind.interner.resolve(*key);
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::ThisOutsideInstance,
                            format!("static method '{key_str}' cannot use 'this' or 'super': no receiver"),
                        )
                        .with_range(*range),
                    );
                }
                ClassMember::Method {
                    key,
                    decorators,
                    body: Some(body),
                    params,
                    range,
                    ..
                } if !decorators.is_empty() => {
                    let key_str = bind.interner.resolve(*key);
                    self.check_decorator_signatures(
                        rec,
                        decorators,
                        super::decorator_signature::DecoratorTarget::Method,
                        key_str,
                        bind,
                    );
                    if super::decorator_receiver::method_uses_receiver(
                        self.ast_arena,
                        params,
                        *body,
                    ) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::InvalidDecoratorTarget,
                                format!("decorated method '{key_str}' cannot use 'this' or 'super': the wrapper loses the receiver"),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Constructor {
                    decorators,
                    body,
                    params,
                    range,
                    ..
                } if !decorators.is_empty() => {
                    self.check_decorator_signatures(
                        rec,
                        decorators,
                        super::decorator_signature::DecoratorTarget::Constructor,
                        "constructor",
                        bind,
                    );
                    if super::decorator_receiver::method_uses_receiver(
                        self.ast_arena,
                        params,
                        *body,
                    ) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::InvalidDecoratorTarget,
                                "decorated constructor cannot use 'this' or 'super': the wrapper loses the receiver".to_owned(),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Getter {
                    key,
                    decorators,
                    body: Some(body),
                    range,
                    ..
                } if !decorators.is_empty() => {
                    let key_str = bind.interner.resolve(*key);
                    self.check_decorator_signatures(
                        rec,
                        decorators,
                        super::decorator_signature::DecoratorTarget::Getter,
                        key_str,
                        bind,
                    );
                    if super::decorator_receiver::method_uses_receiver(self.ast_arena, &[], *body) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::InvalidDecoratorTarget,
                                format!("decorated getter '{key_str}' cannot use 'this' or 'super': the wrapper loses the receiver"),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Setter {
                    key,
                    decorators,
                    body: Some(body),
                    param,
                    range,
                    ..
                } if !decorators.is_empty() => {
                    let key_str = bind.interner.resolve(*key);
                    self.check_decorator_signatures(
                        rec,
                        decorators,
                        super::decorator_signature::DecoratorTarget::Setter,
                        key_str,
                        bind,
                    );
                    if super::decorator_receiver::method_uses_receiver(
                        self.ast_arena,
                        std::slice::from_ref(param),
                        *body,
                    ) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::InvalidDecoratorTarget,
                                format!("decorated setter '{key_str}' cannot use 'this' or 'super': the wrapper loses the receiver"),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Constructor { .. }
                | ClassMember::Destructor { .. }
                | ClassMember::Method { .. }
                | ClassMember::Property { .. }
                | ClassMember::Getter { .. }
                | ClassMember::Setter { .. }
                | ClassMember::StaticBlock { .. } => {}
            }
        }
    }
}
