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
pub(super) struct Scope {
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

    pub fn alloc_temp(&mut self) -> LocalId {
        let f = self.frames.last_mut().unwrap();
        let id = LocalId(f.next_local);
        f.next_local += 1;
        id
    }

    pub(super) fn is_global(&self) -> bool {
        self.frames.len() == 1
    }

    /// Look up a name in the current frame only (no upvalue capture). Used to
    /// decide static self-recursion, mirroring legacy `name_resolves_locally`.
    pub(super) fn resolve_in_current_frame(&self, name: &str) -> Option<HirBinding> {
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
                        let mut names = Vec::new();
                        collect_pattern_identifiers(&d.id, &mut names);
                        for name in names {
                            globals.insert(name, ());
                        }
                    }
                }
                Decl::Class(c) => {
                    let id = c.id.clone().unwrap_or_else(|| Rc::from("anonymous"));
                    globals.insert(id, ());
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
                            let mut names = Vec::new();
                            collect_pattern_identifiers(&d.id, &mut names);
                            for name in names {
                                globals.insert(name, ());
                            }
                        }
                    }
                    _ => {}
                },
                // Type-only declarations are erased at codegen (no bytecode),
                // exactly as legacy `stmt.rs` does.
                Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => {}
                // Namespaces and extensions define their members as globals at
                // lowering time (handled in pass 2); no hoisting needed here.
                Decl::Namespace(_) | Decl::Extension(_) => {}
                _ => return Err(OptError::Unsupported("hir: top-level decl (hoist)")),
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
                    for d in &v.declarators {
                        let value = match &d.init {
                            Some(e) => lo.lower_expr(e, &mut module_scope)?,
                            None => HirExpr::Null,
                        };
                        lo.desugar_pattern_global(&d.id, value, &mut module_scope, &mut top_body)?;
                    }
                }
                Decl::Class(cl) => {
                    let name = cl.id.clone().unwrap_or_else(|| Rc::from("anonymous"));
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
                Decl::Namespace(ns) => {
                    lo.lower_namespace(ns, &mut module_scope, &mut top_body)?;
                }
                Decl::Extension(ext) => {
                    lo.lower_extension(ext, &mut module_scope, &mut top_body)?;
                }
                Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => {}
                _ => return Err(OptError::Unsupported("hir: top-level decl")),
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
        has_rest: false,
        is_async: false,
        is_generator: false,
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
        AssignOp::PowAssign => HirBinOp::Pow,
        AssignOp::BitAndAssign => HirBinOp::BitAnd,
        AssignOp::BitOrAssign => HirBinOp::BitOr,
        AssignOp::BitXorAssign => HirBinOp::BitXor,
        AssignOp::ShlAssign => HirBinOp::Shl,
        AssignOp::ShrAssign => HirBinOp::Shr,
        AssignOp::UShrAssign => HirBinOp::Ushr,
        _ => return Err(OptError::Unsupported("hir: compound assignment op")),
    })
}

fn un_op(op: UnaryOp) -> R<HirUnOp> {
    Ok(match op {
        UnaryOp::Minus => HirUnOp::Neg,
        UnaryOp::Not => HirUnOp::Not,
        UnaryOp::BitNot => HirUnOp::BitNot,
        UnaryOp::Typeof => HirUnOp::Typeof,
        // `Plus` is handled transparently in `lower_expr`.
        _ => return Err(OptError::Unsupported("hir: unary op")),
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

impl<'a> Lowerer<'a> {
    pub(super) fn desugar_pattern_local(
        &mut self,
        pat: &Pattern,
        src: HirExpr,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        match pat {
            Pattern::Identifier { name, .. } => {
                let local = scope.alloc_local(name.clone());
                out.push(HirStmt::Let {
                    local,
                    value: src,
                    ty: HirType::Dynamic,
                });
            }
            Pattern::Array { elements, rest, .. } => {
                let tmp = scope.alloc_temp();
                out.push(HirStmt::Let {
                    local: tmp,
                    value: src,
                    ty: HirType::Dynamic,
                });
                let tmp_expr = HirExpr::Var(HirBinding::Local(tmp));
                for (i, el) in elements.iter().enumerate() {
                    if let Some(elem) = el {
                        let index_expr = HirExpr::Index {
                            object: Box::new(tmp_expr.clone()),
                            index: Box::new(HirExpr::Int(i as i64)),
                            ty: HirType::Dynamic,
                        };
                        self.desugar_pattern_local(&elem.pattern, index_expr, scope, out)?;
                    }
                }
                if let Some(r) = rest {
                    let slice_expr = HirExpr::MethodCall {
                        recv: Box::new(tmp_expr),
                        name: Rc::from("slice"),
                        args: vec![HirExpr::Int(elements.len() as i64)],
                        ty: HirType::Dynamic,
                    };
                    self.desugar_pattern_local(r, slice_expr, scope, out)?;
                }
            }
            Pattern::Object { properties, rest, .. } => {
                let tmp = scope.alloc_temp();
                out.push(HirStmt::Let {
                    local: tmp,
                    value: src,
                    ty: HirType::Dynamic,
                });
                let tmp_expr = HirExpr::Var(HirBinding::Local(tmp));
                for prop in properties {
                    let prop_expr = HirExpr::MemberMaybe {
                        object: Box::new(tmp_expr.clone()),
                        name: prop.key.clone(),
                        ty: HirType::Dynamic,
                    };
                    self.desugar_pattern_local(&prop.value, prop_expr, scope, out)?;
                }
                if let Some(r) = rest {
                    let rest_expr = HirExpr::ObjectRest {
                        object: Box::new(tmp_expr),
                        skip_keys: properties.iter().map(|p| p.key.clone()).collect(),
                    };
                    self.desugar_pattern_local(r, rest_expr, scope, out)?;
                }
            }
            Pattern::Assignment { left, right, .. } => {
                let tmp = scope.alloc_temp();
                out.push(HirStmt::Let {
                    local: tmp,
                    value: src,
                    ty: HirType::Dynamic,
                });
                let tmp_expr = HirExpr::Var(HirBinding::Local(tmp));
                let is_null = HirExpr::TypeTest {
                    value: Box::new(tmp_expr.clone()),
                    kind: HirTypeTest::IsNull,
                };
                let right_hir = self.lower_expr(right, scope)?;
                let assign = HirStmt::Assign {
                    target: HirBinding::Local(tmp),
                    value: right_hir,
                };
                out.push(HirStmt::If {
                    test: is_null,
                    then_body: vec![assign],
                    else_body: vec![],
                });
                self.desugar_pattern_local(left, tmp_expr, scope, out)?;
            }
            Pattern::Rest { argument, .. } => {
                self.desugar_pattern_local(argument, src, scope, out)?;
            }
        }
        Ok(())
    }

    pub(super) fn desugar_pattern_global(
        &mut self,
        pat: &Pattern,
        src: HirExpr,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        match pat {
            Pattern::Identifier { name, .. } => {
                out.push(HirStmt::Assign {
                    target: HirBinding::Global(name.clone()),
                    value: src,
                });
            }
            Pattern::Array { elements, rest, .. } => {
                let tmp = scope.alloc_temp();
                out.push(HirStmt::Let {
                    local: tmp,
                    value: src,
                    ty: HirType::Dynamic,
                });
                let tmp_expr = HirExpr::Var(HirBinding::Local(tmp));
                for (i, el) in elements.iter().enumerate() {
                    if let Some(elem) = el {
                        let index_expr = HirExpr::Index {
                            object: Box::new(tmp_expr.clone()),
                            index: Box::new(HirExpr::Int(i as i64)),
                            ty: HirType::Dynamic,
                        };
                        self.desugar_pattern_global(&elem.pattern, index_expr, scope, out)?;
                    }
                }
                if let Some(r) = rest {
                    let slice_expr = HirExpr::MethodCall {
                        recv: Box::new(tmp_expr),
                        name: Rc::from("slice"),
                        args: vec![HirExpr::Int(elements.len() as i64)],
                        ty: HirType::Dynamic,
                    };
                    self.desugar_pattern_global(r, slice_expr, scope, out)?;
                }
            }
            Pattern::Object { properties, rest, .. } => {
                let tmp = scope.alloc_temp();
                out.push(HirStmt::Let {
                    local: tmp,
                    value: src,
                    ty: HirType::Dynamic,
                });
                let tmp_expr = HirExpr::Var(HirBinding::Local(tmp));
                for prop in properties {
                    let prop_expr = HirExpr::MemberMaybe {
                        object: Box::new(tmp_expr.clone()),
                        name: prop.key.clone(),
                        ty: HirType::Dynamic,
                    };
                    self.desugar_pattern_global(&prop.value, prop_expr, scope, out)?;
                }
                if let Some(r) = rest {
                    let rest_expr = HirExpr::ObjectRest {
                        object: Box::new(tmp_expr),
                        skip_keys: properties.iter().map(|p| p.key.clone()).collect(),
                    };
                    self.desugar_pattern_global(r, rest_expr, scope, out)?;
                }
            }
            Pattern::Assignment { left, right, .. } => {
                let tmp = scope.alloc_temp();
                out.push(HirStmt::Let {
                    local: tmp,
                    value: src,
                    ty: HirType::Dynamic,
                });
                let tmp_expr = HirExpr::Var(HirBinding::Local(tmp));
                let is_null = HirExpr::TypeTest {
                    value: Box::new(tmp_expr.clone()),
                    kind: HirTypeTest::IsNull,
                };
                let right_hir = self.lower_expr(right, scope)?;
                let assign = HirStmt::Assign {
                    target: HirBinding::Local(tmp),
                    value: right_hir,
                };
                out.push(HirStmt::If {
                    test: is_null,
                    then_body: vec![assign],
                    else_body: vec![],
                });
                self.desugar_pattern_global(left, tmp_expr, scope, out)?;
            }
            Pattern::Rest { argument, .. } => {
                self.desugar_pattern_global(argument, src, scope, out)?;
            }
        }
        Ok(())
    }
}

pub fn collect_pattern_identifiers(pat: &Pattern, names: &mut Vec<Rc<str>>) {
    match pat {
        Pattern::Identifier { name, .. } => {
            names.push(name.clone());
        }
        Pattern::Array { elements, rest, .. } => {
            for el in elements {
                if let Some(elem) = el {
                    collect_pattern_identifiers(&elem.pattern, names);
                }
            }
            if let Some(r) = rest {
                collect_pattern_identifiers(r, names);
            }
        }
        Pattern::Object { properties, rest, .. } => {
            for prop in properties {
                collect_pattern_identifiers(&prop.value, names);
            }
            if let Some(r) = rest {
                collect_pattern_identifiers(r, names);
            }
        }
        Pattern::Assignment { left, .. } => {
            collect_pattern_identifiers(left, names);
        }
        Pattern::Rest { argument, .. } => {
            collect_pattern_identifiers(argument, names);
        }
    }
}
