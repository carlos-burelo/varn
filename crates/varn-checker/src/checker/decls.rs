use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::Decl;
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(crate) fn check_decls(&mut self, rec: &mut Recorder, decls: &[Decl], bind: &BindResult) {
        for decl in decls {
            self.check_decl(rec, decl, bind);
        }
    }

    pub(crate) fn check_decl(&mut self, rec: &mut Recorder, decl: &Decl, bind: &BindResult) {
        match decl {
            Decl::Variable(v) => self.check_variable(rec, v, decl.range(), bind),

            Decl::Function(f) => self.check_function_decl(rec, f, bind),

            Decl::Class(c) => self.check_class(rec, c, bind),

            Decl::Enum(e) => self.check_enum(rec, e, bind),

            Decl::Interface(_) => {
                self.next_child_scope(bind);
            }

            Decl::Extension(ext) => self.check_extension(rec, ext, bind),

            Decl::Namespace(ns) => {
                let saved_scope = self.current_scope;
                if let Some(ns_scope) = self.next_child_scope(bind) {
                    self.current_scope = ns_scope;
                    self.record_scope(rec, ns.range.start.offset);
                }
                self.check_decls(rec, &ns.body, bind);
                self.current_scope = saved_scope;
            }

            Decl::Export(e) => {
                self.check_export(rec, e, bind);
            }

            Decl::TypeAlias(_) | Decl::Import(_) | Decl::Struct(_) | Decl::SumType(_) => {}
        }
    }
}
