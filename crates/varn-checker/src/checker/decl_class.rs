use super::recorder::Recorder;
use super::Checker;
use std::sync::Arc;
use varn_core::ast::ClassMember;
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::types::ClassMemberKind;

impl<'r> Checker<'r> {
    pub(super) fn check_class(
        &mut self,
        rec: &mut Recorder,
        c: &varn_core::ast::ClassDecl,
        bind: &BindResult,
    ) {
        if c.modifiers.is_abstract {
            if let Some(id) = &c.id {
                self.abstract_classes
                    .insert(Arc::from(bind.interner.resolve(*id)));
            }
        }
        let name =
            c.id.map(|id| Arc::from(bind.interner.resolve(id)))
                .unwrap_or_else(|| Arc::from("<anon>"));
        let saved_class = self.current_class.replace(name);
        let saved_scope = self.current_scope;
        if let Some(cls_scope) = self.next_child_scope(bind) {
            self.current_scope = cls_scope;
        }

        let mut superclass_members = Vec::new();
        let mut parent =
            c.id.as_ref()
                .and_then(|cls_id| bind.class_parents.get(bind.interner.resolve(*cls_id)));
        while let Some(p) = parent {
            if let Some(m) = bind.get_class_entry(&p.name).map(|e| e.members.clone()) {
                superclass_members.extend(m);
            }
            parent = bind.class_parents.get(&p.name);
        }

        self.check_class_decorators(rec, c, bind);
        self.check_class_overrides(c, &superclass_members, bind);
        self.check_class_members(rec, c, bind);

        self.current_scope = saved_scope;
        self.current_class = saved_class;
    }

    fn check_class_overrides(
        &mut self,
        c: &varn_core::ast::ClassDecl,
        superclass_members: &[varn_sem::types::ClassMemberInfo],
        bind: &BindResult,
    ) {
        for member in &c.body {
            match member {
                ClassMember::Method {
                    key,
                    modifiers,
                    range,
                    ..
                }
                | ClassMember::Getter {
                    key,
                    modifiers,
                    range,
                    ..
                }
                | ClassMember::Setter {
                    key,
                    modifiers,
                    range,
                    ..
                } => {
                    let is_override = modifiers.is_override;
                    let key_str = bind.interner.resolve(*key);
                    let exists_in_superclass = superclass_members.iter().any(|m| {
                        m.name.as_ref() == key_str && m.kind != ClassMemberKind::Constructor
                    });
                    if exists_in_superclass {
                        if !is_override {
                            self.emit(
                                Diagnostic::error(
                                    ErrorCode::MissingOverride,
                                    format!("member '{}' overrides a member in the superclass but is missing the 'override' modifier", key_str),
                                )
                                .with_range(*range),
                            );
                        }
                    } else if is_override {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::SpuriousOverride,
                                format!("member '{}' is marked as override but does not override any member in the superclass", key_str),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Constructor { .. }
                | ClassMember::Destructor { .. }
                | ClassMember::Property { .. }
                | ClassMember::StaticBlock { .. } => {}
            }
        }
    }
}
