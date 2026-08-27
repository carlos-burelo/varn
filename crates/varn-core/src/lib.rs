// Varn Core Architecture Crate (v2)
pub mod ast;
pub mod cg_ty;
pub mod diagnostics;
pub mod doc;
pub mod intrinsic_ops;
pub mod intrinsics;
pub mod kinds;
pub mod module_id;
pub mod numeric;
pub mod op_id;
pub mod op_meta;
pub mod opcode;
pub mod paths;
pub mod source;
pub mod term;
pub mod token;
pub mod trivia;
pub mod type_tag;
pub mod typed_ir;
pub mod well_known;

pub use ast::AstId;
pub use doc::DocComment;

pub use diagnostics::{
    Diagnostic, DiagnosticBag, DiagnosticKind, ErrorCode, RelatedInformation, Suggestion,
};
pub use kinds::TypeKind;
pub use opcode::OpCode;
pub use source::{SourceLocation, SourceRange};

pub use cg_ty::CgTy;
pub use intrinsics::{IntrinsicType, MemberKey};
pub use module_id::{ImportSpecifier, ModuleId};
pub use numeric::{binary_operand_kind, binary_result_kind, wrap_i48, NumericOperand};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
pub use term::{chalk, chalk_fmt, Chalk};
pub use token::{ParsedNumber, Token, TokenKind};
pub use trivia::{Trivia, TriviaKind};
pub use type_tag::{TypeTag, VmValuePayload};
pub use typed_ir::{AnnKey, ExprAnnotation, NumericKind, TypeAnnotations};

/// Version of the runtime:* host API surface. Bump on any breaking change
/// (signature change, symbol removal). Additive changes do NOT bump — a std
/// bundle using a new symbol on an old binary fails at import resolution
/// with a clear module error (spec §3).
pub const HOST_API_VERSION: u32 = 3;

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
