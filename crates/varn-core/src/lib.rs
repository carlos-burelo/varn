pub mod ast;
pub mod doc;
pub mod error;
pub mod intrinsics;
pub mod kinds;
pub mod module_id;
pub mod op_meta;
pub mod opcode;
pub mod paths;
pub mod source;
pub mod stdlib;
pub mod time;
pub mod token;
pub mod typed_ir;
pub mod well_known;

pub use ast::{assign_ast_ids, AstId};
pub use doc::DocComment;

pub use error::{
    Diagnostic, DiagnosticBag, DiagnosticKind, ErrorCode, RelatedInformation, Suggestion,
};
pub use kinds::TypeKind;
pub use opcode::OpCode;
pub use source::{SourceLocation, SourceRange};

pub mod tag_ext;
pub use intrinsics::{IntrinsicType, MemberKey, RuntimeTypeName};
pub use module_id::{ImportSpecifier, ModuleId};
pub use tag_ext::TypeTagExt;
pub use token::{Token, TokenKind};
pub use typed_ir::{NumericKind, TypeAnnotations};
pub use varn_base::TypeTag;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

thread_local! {
    static INTERNER: RefCell<HashSet<Rc<str>>> = RefCell::new(HashSet::new());
}

pub fn intern_string(s: &str) -> Rc<str> {
    INTERNER.with(|interner| {
        let mut interner = interner.borrow_mut();
        if let Some(rc) = interner.get(s) {
            rc.clone()
        } else {
            let rc: Rc<str> = Rc::from(s);
            interner.insert(rc.clone());
            rc
        }
    })
}
