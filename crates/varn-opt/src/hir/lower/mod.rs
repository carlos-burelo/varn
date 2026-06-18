//! AST -> HIR lowering for the imperative core.
//!
//! Handles: top-level function declarations (each becomes a module global),
//! top-level statements, and inside functions — `let`/assignment/`return`/`if`/
//! `while`/`for`, calls, typed binary/unary ops, literals, and identifier
//! resolution (param / local / global). Anything outside this core returns
//! `OptError::Unsupported`, and the whole program falls back to legacy codegen
//! (the supported subset grows stage by stage).
//!
//! Lowering is split by domain: declarations (`decl`), statements (`stmt`), and
//! expressions/`match`/resolution (`expr`). This file holds the lexical state
//! (`Scope`/`Frame`), the `Lowerer` struct, the program entry point, and the
//! small AST→HIR mapping helpers shared across the submodules.

use std::rc::Rc;

use varn_core::ast::decl::{ExportDecl, ImportSpecifier};
use varn_core::ast::operators::{AssignOp, BinaryOp, LogicalOp, UnaryOp, UpdateOp};
use varn_core::ast::{Decl, Expr, Param, Pattern, Stmt, StmtKind};
use varn_core::{NumericKind, TypeAnnotations};

use crate::hir::*;
use crate::{OptError, OptInput};

mod decl;
mod expr;
mod stmt;

type R<T> = Result<T, OptError>;

fn unsupported<T>(what: &'static str) -> R<T> {
    Err(OptError::Unsupported(what))
}

/// One function's lexical state: a stack of block scopes, its local counter,
/// the upvalues it captures from enclosing frames, and — per block — the
/// capture targets that were closed over (so a block pop knows what to
/// `CloseUpvalue`).
struct Frame {
    blocks: Vec<rustc_hash::FxHashMap<Rc<str>, HirBinding>>,
    captured: Vec<Vec<CaptureTarget>>,
    /// Per-block `using` resources (local + whether `disposeAsync`), disposed in
    /// reverse declaration order when the block exits (legacy `pop_scope`).
    disposables: Vec<Vec<(LocalId, bool)>>,
    next_local: u32,
    upvalues: Vec<HirUpvalueSrc>,
}

impl Frame {
    fn new() -> Self {
        Self {
            blocks: vec![rustc_hash::FxHashMap::default()],
            captured: vec![Vec::new()],
            disposables: vec![Vec::new()],
            next_local: 0,
            upvalues: Vec::new(),
        }
    }
}

/// A stack of function frames (innermost last). Name resolution walks outward
/// across frames, capturing enclosing-function bindings as upvalues — mirroring
/// the legacy `Compiler`'s `resolve_upvalue`/`add_upvalue` chain, but in terms
/// of symbolic capture sources since registers are assigned later.
struct Scope {
    frames: Vec<Frame>,
}

impl Scope {
    fn new() -> Self {
        Self {
            frames: vec![Frame::new()],
        }
    }

    fn push_frame(&mut self) {
        self.frames.push(Frame::new());
    }

    /// Pop the innermost frame, returning its local count, captured upvalues,
    /// the capture targets left in its outermost block (params + top-level
    /// locals to close at function end), and that block's `using` resources.
    fn pop_frame(&mut self) -> (u32, Vec<HirUpvalueSrc>, Vec<CaptureTarget>, Vec<(LocalId, bool)>) {
        let mut f = self.frames.pop().expect("frame underflow");
        let block0 = f.captured.pop().unwrap_or_default();
        let disp0 = f.disposables.pop().unwrap_or_default();
        (f.next_local, f.upvalues, block0, disp0)
    }

    fn push_block(&mut self) {
        let f = self.frames.last_mut().unwrap();
        f.blocks.push(rustc_hash::FxHashMap::default());
        f.captured.push(Vec::new());
        f.disposables.push(Vec::new());
    }

    /// Pop the innermost block of the current frame, returning the capture
    /// targets and `using` resources recorded for it.
    fn pop_block(&mut self) -> (Vec<CaptureTarget>, Vec<(LocalId, bool)>) {
        let f = self.frames.last_mut().unwrap();
        f.blocks.pop();
        let captured = f.captured.pop().unwrap_or_default();
        let disposables = f.disposables.pop().unwrap_or_default();
        (captured, disposables)
    }

    /// Record a `using` resource in the current block (disposed on block exit).
    fn record_disposable(&mut self, local: LocalId, is_await: bool) {
        let f = self.frames.last_mut().unwrap();
        f.disposables.last_mut().unwrap().push((local, is_await));
    }

    fn define(&mut self, name: Rc<str>, binding: HirBinding) {
        let f = self.frames.last_mut().unwrap();
        f.blocks.last_mut().unwrap().insert(name, binding);
    }

    fn alloc_local(&mut self, name: Rc<str>) -> LocalId {
        let f = self.frames.last_mut().unwrap();
        let id = LocalId(f.next_local);
        f.next_local += 1;
        f.blocks.last_mut().unwrap().insert(name, HirBinding::Local(id));
        id
    }

    /// Look up a name in the current frame only (no upvalue capture). Used to
    /// decide static self-recursion, mirroring legacy `name_resolves_locally`.
    fn resolve_in_current_frame(&self, name: &str) -> Option<HirBinding> {
        lookup_in_frame(self.frames.last().unwrap(), name)
    }

    /// Resolve a name to a binding in the current frame, or capture it as an
    /// upvalue from an enclosing frame. `None` ⇒ the caller treats it as a
    /// module global.
    fn resolve(&mut self, name: &str) -> Option<HirBinding> {
        let top = self.frames.len() - 1;
        if let Some(b) = lookup_in_frame(&self.frames[top], name) {
            return Some(b);
        }
        self.resolve_upvalue(top, name)
    }

    fn resolve_upvalue(&mut self, frame_idx: usize, name: &str) -> Option<HirBinding> {
        if frame_idx == 0 {
            return None;
        }
        let parent_idx = frame_idx - 1;
        // Look for a direct binding in the parent frame's blocks.
        let mut found: Option<(usize, HirBinding)> = None;
        for (bi, block) in self.frames[parent_idx].blocks.iter().enumerate().rev() {
            if let Some(b) = block.get(name) {
                found = Some((bi, b.clone()));
                break;
            }
        }
        if let Some((bi, binding)) = found {
            let src = match binding {
                HirBinding::Param(i) => {
                    self.frames[parent_idx].captured[bi].push(CaptureTarget::Param(i));
                    HirUpvalueSrc::ParentParam(i)
                }
                HirBinding::Local(id) => {
                    self.frames[parent_idx].captured[bi].push(CaptureTarget::Local(id));
                    HirUpvalueSrc::ParentLocal(id)
                }
                HirBinding::Upvalue(uv) => HirUpvalueSrc::ParentUpvalue(uv),
                HirBinding::Global(_) => return None,
            };
            let idx = self.add_upvalue(frame_idx, src);
            return Some(HirBinding::Upvalue(idx));
        }
        // Not in the parent directly: have the parent capture it first, then
        // chain a parent-upvalue capture into this frame.
        if let Some(HirBinding::Upvalue(parent_uv)) = self.resolve_upvalue(parent_idx, name) {
            let idx = self.add_upvalue(frame_idx, HirUpvalueSrc::ParentUpvalue(parent_uv));
            return Some(HirBinding::Upvalue(idx));
        }
        None
    }

    fn add_upvalue(&mut self, frame_idx: usize, src: HirUpvalueSrc) -> u32 {
        let ups = &mut self.frames[frame_idx].upvalues;
        if let Some(i) = ups.iter().position(|u| *u == src) {
            return i as u32;
        }
        let i = ups.len() as u32;
        ups.push(src);
        i
    }
}

/// Append a block's exit actions to `out`: dispose `using` resources (reverse
/// declaration order) then close any captured upvalues. Mirrors the order in
/// legacy `pop_scope` (dispose, then `CloseUpvalue`).
fn block_epilogue(out: &mut Vec<HirStmt>, captured: Vec<CaptureTarget>, disposables: Vec<(LocalId, bool)>) {
    for (target, is_await) in disposables.into_iter().rev() {
        out.push(HirStmt::Dispose { target, is_await });
    }
    if !captured.is_empty() {
        out.push(HirStmt::CloseUpvalues(captured));
    }
}

fn lookup_in_frame(frame: &Frame, name: &str) -> Option<HirBinding> {
    for block in frame.blocks.iter().rev() {
        if let Some(b) = block.get(name) {
            return Some(b.clone());
        }
    }
    None
}

pub struct Lowerer<'a> {
    ann: &'a TypeAnnotations,
    /// Names declared as module globals (top-level functions and `let`s), so
    /// identifiers that don't resolve locally can be classified as globals.
    globals: rustc_hash::FxHashMap<Rc<str>, ()>,
    /// Name of the function currently being lowered (`None` at the module top
    /// level), used to recognise statically-resolved self-recursion.
    current_fn: Option<Rc<str>>,
    /// Extension-method call/member maps keyed by AST offset. Sites present in
    /// these are desugared by the legacy codegen into mangled global calls; we
    /// fall back for them until that path is reimplemented.
    extension_calls: &'a rustc_hash::FxHashMap<u32, Rc<str>>,
    extension_members: &'a rustc_hash::FxHashMap<u32, Rc<str>>,
    extension_set_members: &'a rustc_hash::FxHashMap<u32, Rc<str>>,
    /// Module export ordering; an export's name position is its module slot.
    export_names: &'a [Rc<str>],
}

/// A function-like body to lower: a statement block (decl/function-expr) or a
/// bare expression (arrow shorthand `x => expr`).
enum BodyRef<'b> {
    Block(&'b Stmt),
    ExprBody(&'b Expr),
    /// No source body (a synthesised constructor that only runs field inits).
    Empty,
}

/// Lower a whole program to HIR. Top-level function declarations become module
/// globals; remaining top-level statements form the synthetic module function.
pub fn lower_program(input: &OptInput<'_>) -> R<HirModule> {
    let program = input.program;
    let mut globals = rustc_hash::FxHashMap::default();

    // Pass 1: register every top-level function/`let` name as a global so calls
    // and references resolve regardless of declaration order.
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            match decl.as_ref() {
                Decl::Function(f) => {
                    globals.insert(f.id.clone(), ());
                }
                Decl::Variable(v) => {
                    for d in &v.declarators {
                        if let Pattern::Identifier { name, .. } = &d.id {
                            globals.insert(name.clone(), ());
                        } else {
                            return unsupported("top-level destructuring binding");
                        }
                    }
                }
                Decl::Class(c) => {
                    if let Some(id) = &c.id {
                        globals.insert(id.clone(), ());
                    } else {
                        return unsupported("anonymous top-level class");
                    }
                }
                Decl::Enum(e) => {
                    globals.insert(e.id.clone(), ());
                }
                Decl::Import(i) => {
                    for spec in &i.specifiers {
                        let local = match spec {
                            ImportSpecifier::Default { local, .. }
                            | ImportSpecifier::Named { local, .. }
                            | ImportSpecifier::Namespace { local, .. } => local.clone(),
                        };
                        globals.insert(local, ());
                    }
                }
                Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
                    Decl::Function(f) => {
                        globals.insert(f.id.clone(), ());
                    }
                    Decl::Class(c) => {
                        if let Some(id) = &c.id {
                            globals.insert(id.clone(), ());
                        }
                    }
                    Decl::Enum(en) => {
                        globals.insert(en.id.clone(), ());
                    }
                    Decl::Variable(v) => {
                        for d in &v.declarators {
                            if let Pattern::Identifier { name, .. } = &d.id {
                                globals.insert(name.clone(), ());
                            }
                        }
                    }
                    _ => {}
                },
                // Type-only declarations are erased at codegen (no bytecode),
                // exactly as legacy `stmt.rs` does.
                Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => {}
                _ => return unsupported("top-level namespace/extension decl"),
            }
        }
    }

    let mut lo = Lowerer {
        ann: input.annotations,
        globals,
        current_fn: None,
        extension_calls: input.extension_calls,
        extension_members: input.extension_members,
        extension_set_members: input.extension_set_members,
        export_names: &input.export_names,
    };

    // Pass 2: lower each declared function and the module top level. The
    // module's own statements share one persistent `module_scope` (so block
    // locals across top-level statements get distinct slots); each top-level
    // function gets a fresh scope (module names are globals, not upvalues).
    let mut functions = Vec::new();
    let mut top_body = Vec::new();
    let mut module_scope = Scope::new();
    for stmt in &program.body {
        match &stmt.kind {
            StmtKind::Decl(decl) => match decl.as_ref() {
                Decl::Function(f) => {
                    let mut fscope = Scope::new();
                    let (func, _ups) = lo.lower_function(f, &mut fscope)?;
                    functions.push(func);
                }
                Decl::Variable(v) => {
                    // Top-level `let x = e` -> assign module global x.
                    for d in &v.declarators {
                        let name = match &d.id {
                            Pattern::Identifier { name, .. } => name.clone(),
                            _ => return unsupported("top-level destructuring"),
                        };
                        let value = match &d.init {
                            Some(e) => lo.lower_expr(e, &mut module_scope)?,
                            None => HirExpr::Null,
                        };
                        top_body.push(HirStmt::Assign {
                            target: HirBinding::Global(name),
                            value,
                        });
                    }
                }
                Decl::Class(cl) => {
                    let name = match &cl.id {
                        Some(id) => id.clone(),
                        None => return unsupported("anonymous top-level class"),
                    };
                    let hir_class = lo.lower_class(cl, &mut module_scope)?;
                    top_body.push(HirStmt::Assign {
                        target: HirBinding::Global(name),
                        value: HirExpr::Class(Box::new(hir_class)),
                    });
                }
                Decl::Enum(en) => {
                    let hir_enum = lo.lower_enum(en, &mut module_scope)?;
                    top_body.push(HirStmt::Assign {
                        target: HirBinding::Global(en.id.clone()),
                        value: HirExpr::Enum(Box::new(hir_enum)),
                    });
                }
                Decl::Import(i) => {
                    top_body.push(lo.lower_import(i)?);
                }
                Decl::Export(e) => {
                    lo.lower_export(e, &mut module_scope, &mut top_body)?;
                }
                Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => {}
                _ => return unsupported("top-level decl"),
            },
            _ => {
                lo.lower_stmt(stmt, &mut module_scope, &mut top_body)?;
            }
        }
    }

    let top_level = HirFunction {
        name: Rc::from("<module>"),
        params: Vec::new(),
        locals: module_scope.frames[0].next_local,
        body: top_body,
        return_ty: HirType::Dynamic,
        upvalue_count: 0,
        has_this: false,
    };

    Ok(HirModule {
        top_level,
        functions,
    })
}

fn param_ty(_p: &Param) -> HirType {
    HirType::Dynamic
}

fn numeric_ty(ann: &TypeAnnotations, offset: u32) -> HirType {
    match ann.get_numeric(offset) {
        Some(NumericKind::Int) => HirType::Int,
        Some(NumericKind::Float) => HirType::Float,
        _ => HirType::Dynamic,
    }
}

fn bin_op(op: BinaryOp) -> R<HirBinOp> {
    Ok(match op {
        BinaryOp::Add => HirBinOp::Add,
        BinaryOp::Sub => HirBinOp::Sub,
        BinaryOp::Mul => HirBinOp::Mul,
        BinaryOp::Div => HirBinOp::Div,
        BinaryOp::Mod => HirBinOp::Mod,
        BinaryOp::Pow => HirBinOp::Pow,
        BinaryOp::Eq => HirBinOp::Eq,
        BinaryOp::NotEq => HirBinOp::Ne,
        BinaryOp::Lt => HirBinOp::Lt,
        BinaryOp::LtEq => HirBinOp::Le,
        BinaryOp::Gt => HirBinOp::Gt,
        BinaryOp::GtEq => HirBinOp::Ge,
        BinaryOp::BitAnd => HirBinOp::BitAnd,
        BinaryOp::BitOr => HirBinOp::BitOr,
        BinaryOp::BitXor => HirBinOp::BitXor,
        BinaryOp::Shl => HirBinOp::Shl,
        BinaryOp::Shr => HirBinOp::Shr,
        BinaryOp::UShr => HirBinOp::Ushr,
        BinaryOp::Instanceof => HirBinOp::Instanceof,
        BinaryOp::In => HirBinOp::In,
    })
}

fn compound_to_bin(op: AssignOp) -> R<HirBinOp> {
    Ok(match op {
        AssignOp::AddAssign => HirBinOp::Add,
        AssignOp::SubAssign => HirBinOp::Sub,
        AssignOp::MulAssign => HirBinOp::Mul,
        AssignOp::DivAssign => HirBinOp::Div,
        AssignOp::ModAssign => HirBinOp::Mod,
        AssignOp::BitAndAssign => HirBinOp::BitAnd,
        AssignOp::BitOrAssign => HirBinOp::BitOr,
        AssignOp::BitXorAssign => HirBinOp::BitXor,
        AssignOp::ShlAssign => HirBinOp::Shl,
        AssignOp::ShrAssign => HirBinOp::Shr,
        _ => return unsupported("compound assignment op"),
    })
}

fn un_op(op: UnaryOp) -> R<HirUnOp> {
    Ok(match op {
        UnaryOp::Minus => HirUnOp::Neg,
        UnaryOp::Not => HirUnOp::Not,
        UnaryOp::BitNot => HirUnOp::BitNot,
        UnaryOp::Typeof => HirUnOp::Typeof,
        // `Plus` is handled transparently in `lower_expr`.
        _ => return unsupported("unary op"),
    })
}

fn logical_op(op: LogicalOp) -> HirLogicalOp {
    match op {
        LogicalOp::And => HirLogicalOp::And,
        LogicalOp::Or => HirLogicalOp::Or,
        LogicalOp::Nullish => HirLogicalOp::Nullish,
    }
}

fn update_op(op: UpdateOp) -> HirUpdateOp {
    match op {
        UpdateOp::Increment => HirUpdateOp::Inc,
        UpdateOp::Decrement => HirUpdateOp::Dec,
    }
}
