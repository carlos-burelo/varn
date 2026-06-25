pub mod ast;
pub mod doc;
pub mod error;
pub mod intrinsic_ops;
pub mod intrinsics;
pub mod kinds;
pub mod module_id;
pub mod op_id;
pub mod op_meta;
pub mod opcode;
pub mod paths;
pub mod source;
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
pub use token::{ParsedNumber, Token, TokenKind};
pub use typed_ir::{NumericKind, TypeAnnotations};
pub use varn_base::TypeTag;

use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static INTERNER: RefCell<FxHashMap<Box<str>, Rc<str>>> = RefCell::new(FxHashMap::default());
}

pub fn intern_string(s: &str) -> Rc<str> {
    INTERNER.with(|interner| {
        let mut interner = interner.borrow_mut();
        if let Some(rc) = interner.get(s) {
            return rc.clone();
        }
        let rc: Rc<str> = Rc::from(s);
        interner.insert(Box::from(s), rc.clone());
        rc
    })
}

pub fn clear_interner() {
    INTERNER.with(|interner| {
        interner.borrow_mut().clear();
    });
}
