// Varn Core Architecture Crate (v2)
pub mod ast;
pub mod atom;
pub mod cg_ty;
pub mod diagnostics;
pub mod doc;
pub mod errors;
pub mod intrinsic_ops;
pub mod intrinsics;
pub mod kinds;
pub mod module_id;
pub mod numeric;
pub mod numeric_conv;
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
pub use atom::{Atom, AtomInterner};
pub use doc::DocComment;
pub use errors::RuntimeErrorKind;

pub use diagnostics::{
    Diagnostic, DiagnosticBag, DiagnosticKind, ErrorCode, RelatedInformation, Suggestion,
};
pub use kinds::TypeKind;
pub use opcode::OpCode;
pub use source::{SourceLocation, SourceRange};

pub use cg_ty::CgTy;
pub use intrinsics::{IntrinsicType, MemberKey};
pub use module_id::{ImportSpecifier, ModuleId};
pub use numeric::{
    add_int, binary_operand_kind, ceil_div_int, checked_int, div_int, floor_div_int, mod_int, mul_int, neg_int, pow_int, rem_int,
    sub_int, IntDivFault, NumericOperand, INT_MAX, INT_MIN,
};
pub use numeric_conv::{float_to_int, NumConv, NumericDomain};
pub use term::{chalk, chalk_fmt, Chalk};
pub use token::{ParsedNumber, Token, TokenKind};
pub use trivia::{Trivia, TriviaKind};
pub use type_tag::{FieldRepr, TypeTag, VmValuePayload};
pub use typed_ir::{AnnKey, ExprAnnotation, NumericKind, TypeAnnotations};

/// Version of the runtime:* host API surface. Bump on any breaking change
/// (signature change, symbol removal). Additive changes do NOT bump — a std
/// bundle using a new symbol on an old binary fails at import resolution
/// with a clear module error (spec §3).
pub const HOST_API_VERSION: u32 = 3;

/// No-op kept for API compatibility: `varn-lsp`/`varn-pipeline` call this on
/// every module invalidation. It used to clear a `thread_local` `Arc<str>`
/// interner (`intern_string`, now removed -- it had no remaining callers);
/// the real per-session interning now lives in `AtomInterner`
/// (`atom.rs`), which is owned data, not global state, and clears with
/// whatever owns it. Safe to remove entirely once those two call sites are
/// updated to stop calling it.
pub fn clear_interner() {}
