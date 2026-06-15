//! AST -> HIR lowering for the imperative core.
//!
//! Handles: top-level function declarations (each becomes a module global),
//! top-level statements, and inside functions — `let`/assignment/`return`/`if`/
//! `while`/`for`, calls, typed binary/unary ops, literals, and identifier
//! resolution (param / local / global). Anything outside this core returns
//! `OptError::Unsupported`, and the whole program falls back to legacy codegen
//! (the supported subset grows stage by stage).

use std::rc::Rc;

use varn_core::ast::decl::{ClassDecl, ClassMember, EnumDecl};
use varn_core::ast::expr::{ArrayEl, ArrowBody, MatchBody, ObjectProp, PropKey};
use varn_core::ast::operators::{AssignOp, BinaryOp, LogicalOp, UnaryOp, UpdateOp};
use varn_core::ast::pattern::MatchPattern;
use varn_core::ast::{
    Arg, Decl, Expr, ExprKind, ForInit, FunctionDecl, Param, Pattern, Stmt, StmtKind,
};
use varn_core::{NumericKind, TypeAnnotations};

use crate::hir::*;
use crate::{OptError, OptInput};

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
    next_local: u32,
    upvalues: Vec<HirUpvalueSrc>,
}

impl Frame {
    fn new() -> Self {
        Self {
            blocks: vec![rustc_hash::FxHashMap::default()],
            captured: vec![Vec::new()],
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
    /// and the capture targets left in its outermost block (params + top-level
    /// locals to close at function end).
    fn pop_frame(&mut self) -> (u32, Vec<HirUpvalueSrc>, Vec<CaptureTarget>) {
        let mut f = self.frames.pop().expect("frame underflow");
        let block0 = f.captured.pop().unwrap_or_default();
        (f.next_local, f.upvalues, block0)
    }

    fn push_block(&mut self) {
        let f = self.frames.last_mut().unwrap();
        f.blocks.push(rustc_hash::FxHashMap::default());
        f.captured.push(Vec::new());
    }

    /// Pop the innermost block of the current frame, returning the capture
    /// targets recorded for it.
    fn pop_block(&mut self) -> Vec<CaptureTarget> {
        let f = self.frames.last_mut().unwrap();
        f.blocks.pop();
        f.captured.pop().unwrap_or_default()
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
                // Type-only declarations are erased at codegen (no bytecode),
                // exactly as legacy `stmt.rs` does.
                Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => {}
                _ => return unsupported("top-level namespace decl"),
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

/// A function-like body to lower: a statement block (decl/function-expr) or a
/// bare expression (arrow shorthand `x => expr`).
enum BodyRef<'b> {
    Block(&'b Stmt),
    ExprBody(&'b Expr),
    /// No source body (a synthesised constructor that only runs field inits).
    Empty,
}

impl<'a> Lowerer<'a> {
    /// Lower a function declaration into its `HirFunction` plus the upvalues it
    /// captures from enclosing frames. `scope` carries the enclosing frame
    /// stack so nested functions can capture; top-level callers pass a fresh
    /// `Scope` (its empty frame 0 yields no captures → globals).
    fn lower_function(
        &mut self,
        f: &FunctionDecl,
        scope: &mut Scope,
    ) -> R<(HirFunction, Vec<HirUpvalueSrc>)> {
        self.lower_function_like(
            f.id.clone(),
            &f.params,
            f.modifiers.is_async,
            f.modifiers.is_generator,
            !f.type_params.is_empty(),
            false,
            BodyRef::Block(&f.body),
            &[],
            scope,
        )
    }

    /// Lower a class declaration to a `HirClass` (core subset: no inheritance,
    /// static members, accessors, or decorators — those fall back to legacy).
    fn lower_class(&mut self, decl: &ClassDecl, scope: &mut Scope) -> R<HirClass> {
        let name = decl.id.clone().unwrap_or_else(|| Rc::from("anonymous"));
        if decl.super_class.is_some() {
            return unsupported("class inheritance");
        }
        if !decl.decorators.is_empty() {
            return unsupported("class decorators");
        }
        if !decl.type_params.is_empty() {
            return unsupported("generic class");
        }
        if decl.modifiers.is_abstract {
            return unsupported("abstract class");
        }

        let mut fields: Vec<Rc<str>> = Vec::new();
        let mut field_inits: Vec<(Rc<str>, &Expr)> = Vec::new();
        let mut ctor_member: Option<(&[Param], &Stmt)> = None;
        let mut methods_ast: Vec<(Rc<str>, &[Param], &Stmt)> = Vec::new();

        for member in &decl.body {
            match member {
                ClassMember::Property {
                    key,
                    init,
                    modifiers,
                    ..
                } => {
                    if modifiers.is_static {
                        return unsupported("static field");
                    }
                    fields.push(key.clone());
                    if let Some(e) = init {
                        field_inits.push((key.clone(), e));
                    }
                }
                ClassMember::Constructor { params, body, .. } => {
                    ctor_member = Some((params, body));
                }
                ClassMember::Method {
                    key,
                    params,
                    body: Some(body),
                    modifiers,
                    decorators,
                    ..
                } => {
                    if modifiers.is_static {
                        return unsupported("static method");
                    }
                    if modifiers.is_async || modifiers.is_generator {
                        return unsupported("async/generator method");
                    }
                    if !decorators.is_empty() {
                        return unsupported("method decorators");
                    }
                    methods_ast.push((key.clone(), params, body));
                }
                ClassMember::Method { body: None, .. } => return unsupported("abstract method"),
                ClassMember::Getter { .. } | ClassMember::Setter { .. } => {
                    return unsupported("class accessor")
                }
                ClassMember::StaticBlock { .. } => return unsupported("static block"),
                ClassMember::Destructor { .. } => return unsupported("class destructor"),
            }
        }

        // Constructor (synthesised when absent); field inits run after its body.
        let (ctor_func, ctor_ups) = match ctor_member {
            Some((params, body)) => self.lower_function_like(
                Rc::from("constructor"),
                params,
                false,
                false,
                false,
                true,
                BodyRef::Block(body),
                &field_inits,
                scope,
            )?,
            None => self.lower_function_like(
                Rc::from("constructor"),
                &[],
                false,
                false,
                false,
                true,
                BodyRef::Empty,
                &field_inits,
                scope,
            )?,
        };

        let mut methods = Vec::new();
        for (key, params, body) in methods_ast {
            let (func, upvalues) = self.lower_function_like(
                key.clone(),
                params,
                false,
                false,
                false,
                true,
                BodyRef::Block(body),
                &[],
                scope,
            )?;
            methods.push(HirMethod {
                key,
                func,
                upvalues,
            });
        }

        Ok(HirClass {
            name,
            fields,
            ctor: HirMethod {
                key: Rc::from("constructor"),
                func: ctor_func,
                upvalues: ctor_ups,
            },
            methods,
        })
    }

    /// Lower an enum declaration. Variants become `MakeEnumVariant` statics on
    /// a class; instance fields/methods mirror the class core. (Core subset: no
    /// static members, field initializers, or accessors.)
    fn lower_enum(&mut self, decl: &EnumDecl, scope: &mut Scope) -> R<HirEnum> {
        if !decl.type_params.is_empty() {
            return unsupported("generic enum");
        }
        let name = decl.id.clone();
        let mut variants = Vec::new();
        let mut tag = 0i64;
        for member in &decl.members {
            if let Some(init) = &member.init {
                match &init.kind {
                    ExprKind::IntLiteral { value, .. } => tag = *value,
                    _ => return unsupported("non-integer enum discriminant"),
                }
            }
            let fields_str = member
                .payload_fields
                .iter()
                .map(|f| f.name.as_ref())
                .collect::<Vec<&str>>()
                .join(",");
            let meta = if fields_str.is_empty() {
                format!("{}.{}", name, member.id)
            } else {
                format!("{}.{}:{}", name, member.id, fields_str)
            };
            variants.push(HirEnumVariant {
                name: member.id.clone(),
                tag,
                meta: Rc::from(meta.as_str()),
            });
            tag += 1;
        }

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        for member in &decl.body {
            match member {
                ClassMember::Property {
                    key,
                    init,
                    modifiers,
                    ..
                } => {
                    if modifiers.is_static {
                        return unsupported("static enum field");
                    }
                    if init.is_some() {
                        return unsupported("enum field initializer");
                    }
                    fields.push(key.clone());
                }
                ClassMember::Method {
                    key,
                    params,
                    body: Some(body),
                    modifiers,
                    decorators,
                    ..
                } => {
                    if modifiers.is_static {
                        return unsupported("static enum method");
                    }
                    if modifiers.is_async || modifiers.is_generator {
                        return unsupported("async/generator enum method");
                    }
                    if !decorators.is_empty() {
                        return unsupported("enum method decorators");
                    }
                    let (func, upvalues) = self.lower_function_like(
                        key.clone(),
                        params,
                        false,
                        false,
                        false,
                        true,
                        BodyRef::Block(body),
                        &[],
                        scope,
                    )?;
                    methods.push(HirMethod {
                        key: key.clone(),
                        func,
                        upvalues,
                    });
                }
                _ => return unsupported("enum member kind"),
            }
        }

        Ok(HirEnum {
            name,
            variants,
            fields,
            methods,
        })
    }

    /// Lower a `match` expression to a `HirExpr::Match`. Each case allocates its
    /// pattern bindings as locals in a per-case block scope, then lowers the arm
    /// body. Guards and record/sequence/type patterns fall back to legacy.
    fn lower_match(
        &mut self,
        subject: &Expr,
        cases: &[varn_core::ast::expr::MatchCase],
        scope: &mut Scope,
    ) -> R<HirExpr> {
        let subject = Box::new(self.lower_expr(subject, scope)?);
        let mut hcases = Vec::with_capacity(cases.len());
        for case in cases {
            if case.guard.is_some() {
                return unsupported("match guard");
            }
            scope.push_block();
            let test_res = self.lower_case_test(&case.pattern, scope);
            let test = match test_res {
                Ok(t) => t,
                Err(e) => {
                    scope.pop_block();
                    return Err(e);
                }
            };
            let mut body = Vec::new();
            let result = match &case.body {
                MatchBody::Block(s) => {
                    match &s.kind {
                        StmtKind::Block { stmts } => {
                            for st in stmts {
                                self.lower_stmt(st, scope, &mut body)?;
                            }
                        }
                        _ => self.lower_stmt(s, scope, &mut body)?,
                    }
                    None
                }
                MatchBody::Expr(e) => Some(self.lower_expr(e, scope)?),
            };
            let captured = scope.pop_block();
            if !captured.is_empty() {
                body.push(HirStmt::CloseUpvalues(captured));
            }
            hcases.push(HirMatchCase { test, body, result });
        }
        Ok(HirExpr::Match {
            subject,
            cases: hcases,
        })
    }

    fn lower_case_test(&mut self, pat: &MatchPattern, scope: &mut Scope) -> R<HirCaseTest> {
        Ok(match pat {
            MatchPattern::Wildcard => HirCaseTest::Wildcard,
            MatchPattern::Literal(lit) => HirCaseTest::Literal(self.lower_expr(lit, scope)?),
            MatchPattern::Identifier(name) => HirCaseTest::Bind(scope.alloc_local(name.clone())),
            MatchPattern::EnumVariant {
                variant_name,
                bindings,
                ..
            } => {
                let mut binds = Vec::with_capacity(bindings.len());
                for b in bindings {
                    if &*b.name == "_" {
                        binds.push(None);
                    } else {
                        binds.push(Some(scope.alloc_local(b.name.clone())));
                    }
                }
                HirCaseTest::EnumVariant {
                    name: variant_name.clone(),
                    binds,
                }
            }
            _ => return unsupported("match pattern kind"),
        })
    }

    /// Shared lowering for declarations, function expressions, arrows, and class
    /// methods/constructors. `has_this` marks register 0 as the receiver;
    /// `field_inits` are `this.name = expr` assignments appended after the body
    /// (constructor field initializers, lowered in the constructor's frame —
    /// matching legacy ordering).
    #[allow(clippy::too_many_arguments)]
    fn lower_function_like(
        &mut self,
        name: Rc<str>,
        params_ast: &[Param],
        is_async: bool,
        is_generator: bool,
        generic: bool,
        has_this: bool,
        body: BodyRef<'_>,
        field_inits: &[(Rc<str>, &Expr)],
        scope: &mut Scope,
    ) -> R<(HirFunction, Vec<HirUpvalueSrc>)> {
        if is_async || is_generator {
            return unsupported("async/generator function");
        }
        if generic {
            return unsupported("generic function");
        }
        scope.push_frame();
        let prev_fn = self.current_fn.take();
        self.current_fn = Some(name.clone());

        let built = self.lower_function_body(params_ast, body, field_inits, scope);

        self.current_fn = prev_fn;
        let (params, body) = match built {
            Ok(v) => v,
            Err(e) => {
                scope.pop_frame(); // keep the frame stack balanced on error
                return Err(e);
            }
        };
        // Function-level captures (params + top-level locals) are closed by the
        // VM's `Return` (close_upvalues_above base); only inner blocks need an
        // explicit `CloseUpvalue`, emitted in `lower_block`/`Block`/`For`.
        let (locals, upvalues, _captured0) = scope.pop_frame();
        let func = HirFunction {
            name,
            params,
            locals,
            body,
            return_ty: HirType::Dynamic,
            upvalue_count: upvalues.len() as u32,
            has_this,
        };
        Ok((func, upvalues))
    }

    fn lower_function_body(
        &mut self,
        params_ast: &[Param],
        body: BodyRef<'_>,
        field_inits: &[(Rc<str>, &Expr)],
        scope: &mut Scope,
    ) -> R<(Vec<HirParam>, Vec<HirStmt>)> {
        let mut params = Vec::new();
        for (i, p) in params_ast.iter().enumerate() {
            if p.is_rest || p.is_optional || p.default.is_some() {
                return unsupported("rest/optional/default param");
            }
            let pname = match &p.pattern {
                Pattern::Identifier { name, .. } => name.clone(),
                _ => return unsupported("destructuring param"),
            };
            scope.define(pname.clone(), HirBinding::Param(i as u32));
            params.push(HirParam {
                name: pname,
                ty: param_ty(p),
            });
        }
        let mut out = Vec::new();
        match body {
            BodyRef::Block(stmt) => match &stmt.kind {
                StmtKind::Block { stmts } => {
                    for s in stmts {
                        self.lower_stmt(s, scope, &mut out)?;
                    }
                }
                _ => self.lower_stmt(stmt, scope, &mut out)?,
            },
            BodyRef::ExprBody(e) => {
                let v = self.lower_expr(e, scope)?;
                out.push(HirStmt::Return(Some(v)));
            }
            BodyRef::Empty => {}
        }
        // Constructor field initializers run after the body (legacy order).
        for (fname, fexpr) in field_inits {
            let value = self.lower_expr(fexpr, scope)?;
            out.push(HirStmt::SetMember {
                object: HirExpr::This,
                name: fname.clone(),
                value,
            });
        }
        Ok((params, out))
    }

    fn lower_block(&mut self, stmt: &Stmt, scope: &mut Scope) -> R<Vec<HirStmt>> {
        let mut out = Vec::new();
        scope.push_block();
        match &stmt.kind {
            StmtKind::Block { stmts } => {
                for s in stmts {
                    self.lower_stmt(s, scope, &mut out)?;
                }
            }
            _ => self.lower_stmt(stmt, scope, &mut out)?,
        }
        let captured = scope.pop_block();
        if !captured.is_empty() {
            out.push(HirStmt::CloseUpvalues(captured));
        }
        Ok(out)
    }

    fn lower_stmt(&mut self, stmt: &Stmt, scope: &mut Scope, out: &mut Vec<HirStmt>) -> R<()> {
        match &stmt.kind {
            StmtKind::Empty => {}
            StmtKind::Block { stmts } => {
                scope.push_block();
                for s in stmts {
                    self.lower_stmt(s, scope, out)?;
                }
                let captured = scope.pop_block();
                if !captured.is_empty() {
                    out.push(HirStmt::CloseUpvalues(captured));
                }
            }
            StmtKind::Expr { expression } => {
                if let ExprKind::Assign { op, target, value } = &expression.kind {
                    // Member/index assignment target (simple `=` only).
                    if let ExprKind::Member {
                        object,
                        property,
                        computed,
                        optional,
                    } = &target.kind
                    {
                        if *optional {
                            return unsupported("optional assignment target");
                        }
                        if !matches!(op, AssignOp::Assign) {
                            return unsupported("compound member assignment");
                        }
                        let off = target.range.start.offset;
                        if self.ann.get_slot_idx(off).is_some() {
                            return unsupported("module-slot assignment");
                        }
                        if self.extension_set_members.contains_key(&off) {
                            return unsupported("extension setter");
                        }
                        if matches!(object.kind, ExprKind::Super) {
                            return unsupported("super assignment");
                        }
                        let object_hir = self.lower_expr(object, scope)?;
                        if *computed {
                            let index = self.lower_expr(property, scope)?;
                            let value = self.lower_expr(value, scope)?;
                            out.push(HirStmt::SetIndex {
                                object: object_hir,
                                index,
                                value,
                            });
                        } else {
                            let name = match &property.kind {
                                ExprKind::Identifier { name } => name.clone(),
                                _ => return unsupported("non-identifier property assign"),
                            };
                            let value = self.lower_expr(value, scope)?;
                            out.push(HirStmt::SetMember {
                                object: object_hir,
                                name,
                                value,
                            });
                        }
                        return Ok(());
                    }
                    let binding = match &target.kind {
                        ExprKind::Identifier { name } => self.resolve(name, scope),
                        _ => return unsupported("non-identifier assign target"),
                    };
                    let val_expr = self.lower_expr(value, scope)?;
                    let value = match op {
                        AssignOp::Assign => val_expr,
                        _ => {
                            let bop = compound_to_bin(*op)?;
                            let ty = numeric_ty(self.ann, expression.range.start.offset);
                            HirExpr::Binary {
                                op: bop,
                                lhs: Box::new(HirExpr::Var(binding.clone())),
                                rhs: Box::new(val_expr),
                                ty,
                            }
                        }
                    };
                    out.push(HirStmt::Assign {
                        target: binding,
                        value,
                    });
                } else {
                    let e = self.lower_expr(expression, scope)?;
                    out.push(HirStmt::Expr(e));
                }
            }
            StmtKind::Decl(decl) => match decl.as_ref() {
                Decl::Variable(v) => {
                    for d in &v.declarators {
                        let name = match &d.id {
                            Pattern::Identifier { name, .. } => name.clone(),
                            _ => return unsupported("destructuring let"),
                        };
                        let value = match &d.init {
                            Some(e) => self.lower_expr(e, scope)?,
                            None => HirExpr::Null,
                        };
                        let local = scope.alloc_local(name);
                        out.push(HirStmt::Let {
                            local,
                            value,
                            ty: HirType::Dynamic,
                        });
                    }
                }
                // Nested function declaration → closure bound to a local. Bind
                // the name *before* lowering the body so the body can capture
                // itself (recursion via an open upvalue on this local slot).
                Decl::Function(f) => {
                    let local = scope.alloc_local(f.id.clone());
                    let (func, upvalues) = self.lower_function(f, scope)?;
                    out.push(HirStmt::Let {
                        local,
                        value: HirExpr::Closure {
                            func: Box::new(func),
                            upvalues,
                        },
                        ty: HirType::Dynamic,
                    });
                }
                Decl::Class(cl) => {
                    let cname = match &cl.id {
                        Some(id) => id.clone(),
                        None => return unsupported("anonymous nested class"),
                    };
                    let hir_class = self.lower_class(cl, scope)?;
                    let local = scope.alloc_local(cname);
                    out.push(HirStmt::Let {
                        local,
                        value: HirExpr::Class(Box::new(hir_class)),
                        ty: HirType::Dynamic,
                    });
                }
                Decl::Enum(en) => {
                    let hir_enum = self.lower_enum(en, scope)?;
                    let local = scope.alloc_local(en.id.clone());
                    out.push(HirStmt::Let {
                        local,
                        value: HirExpr::Enum(Box::new(hir_enum)),
                        ty: HirType::Dynamic,
                    });
                }
                Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => {}
                _ => return unsupported("nested namespace decl"),
            },
            StmtKind::Return { argument } => {
                let v = match argument {
                    Some(e) => Some(self.lower_expr(e, scope)?),
                    None => None,
                };
                out.push(HirStmt::Return(v));
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let test = self.lower_expr(test, scope)?;
                let then_body = self.lower_block(consequent, scope)?;
                let else_body = match alternate {
                    Some(alt) => self.lower_block(alt, scope)?,
                    None => Vec::new(),
                };
                out.push(HirStmt::If {
                    test,
                    then_body,
                    else_body,
                });
            }
            StmtKind::While { test, body } => {
                let test = self.lower_expr(test, scope)?;
                let body = self.lower_block(body, scope)?;
                out.push(HirStmt::While { test, body });
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                // Desugar `for (init; test; update) body`
                //   -> init; while (test) { body; update }
                scope.push_block();
                if let Some(init) = init {
                    match init.as_ref() {
                        ForInit::Expr(e) => {
                            let e = self.lower_expr(e, scope)?;
                            out.push(HirStmt::Expr(e));
                        }
                        ForInit::Var { declarators, .. } => {
                            for d in declarators {
                                let name = match &d.id {
                                    Pattern::Identifier { name, .. } => name.clone(),
                                    _ => return unsupported("for-init destructuring"),
                                };
                                let value = match &d.init {
                                    Some(e) => self.lower_expr(e, scope)?,
                                    None => HirExpr::Null,
                                };
                                let local = scope.alloc_local(name);
                                out.push(HirStmt::Let {
                                    local,
                                    value,
                                    ty: HirType::Dynamic,
                                });
                            }
                        }
                    }
                }
                let test = match test {
                    Some(t) => self.lower_expr(t, scope)?,
                    None => HirExpr::Bool(true),
                };
                let mut loop_body = self.lower_block(body, scope)?;
                if let Some(u) = update {
                    let u = self.lower_expr(u, scope)?;
                    loop_body.push(HirStmt::Expr(u));
                }
                out.push(HirStmt::While {
                    test,
                    body: loop_body,
                });
                let captured = scope.pop_block();
                if !captured.is_empty() {
                    out.push(HirStmt::CloseUpvalues(captured));
                }
            }
            StmtKind::Break { label: None } => out.push(HirStmt::Break),
            StmtKind::Continue { label: None } => out.push(HirStmt::Continue),
            _ => return unsupported("statement kind"),
        }
        Ok(())
    }

    fn lower_expr(&mut self, expr: &Expr, scope: &mut Scope) -> R<HirExpr> {
        let offset = expr.range.start.offset;
        match &expr.kind {
            ExprKind::IntLiteral { value, .. } => Ok(HirExpr::Int(*value)),
            ExprKind::FloatLiteral { value, .. } => Ok(HirExpr::Float(*value)),
            ExprKind::StrLiteral { value } => Ok(HirExpr::Str(Rc::from(value.as_str()))),
            ExprKind::BoolLiteral { value } => Ok(HirExpr::Bool(*value)),
            ExprKind::NullLiteral => Ok(HirExpr::Null),
            ExprKind::This => Ok(HirExpr::This),
            ExprKind::New { callee, args, .. } => {
                // `new C(args)` compiles to a plain Call; the VM constructs an
                // instance when the callee is a class (legacy `expr/mod.rs`).
                if let Some(mapping) = self.ann.get_call_mapping(offset) {
                    let identity = mapping.len() == args.len()
                        && mapping.iter().enumerate().all(|(i, m)| *m == Some(i));
                    if !identity {
                        return unsupported("new with non-trivial arg mapping");
                    }
                }
                let mut hargs = Vec::with_capacity(args.len());
                for a in args {
                    match a {
                        Arg::Positional(e) => hargs.push(self.lower_expr(e, scope)?),
                        _ => return unsupported("new with named/spread arg"),
                    }
                }
                let callee = Box::new(self.lower_expr(callee, scope)?);
                Ok(HirExpr::Call {
                    callee,
                    args: hargs,
                    ty: HirType::Dynamic,
                })
            }
            ExprKind::Paren { expression } => self.lower_expr(expression, scope),
            ExprKind::Identifier { name } => Ok(HirExpr::Var(self.resolve(name, scope))),
            ExprKind::Binary { op, left, right } => {
                let lhs = Box::new(self.lower_expr(left, scope)?);
                let rhs = Box::new(self.lower_expr(right, scope)?);
                let ty = numeric_ty(self.ann, offset);
                Ok(HirExpr::Binary {
                    op: bin_op(*op)?,
                    lhs,
                    rhs,
                    ty,
                })
            }
            ExprKind::Unary { op, operand, .. } => {
                let operand = Box::new(self.lower_expr(operand, scope)?);
                Ok(HirExpr::Unary {
                    op: un_op(*op)?,
                    operand,
                    ty: HirType::Dynamic,
                })
            }
            ExprKind::Call {
                callee,
                args,
                optional,
                ..
            } => {
                if *optional {
                    return unsupported("optional call");
                }
                // Intrinsics, extension calls, and calls whose arguments need
                // reordering/default-filling (`get_call_mapping`) are desugared
                // by the legacy codegen → fall back until those land.
                if self.ann.get_intrinsic(offset).is_some() {
                    return unsupported("intrinsic call");
                }
                // A non-trivial call mapping reorders args or fills defaults —
                // replicating that is §1.10. An *identity* mapping (param i ← arg
                // i, no gaps) is just plain positional, so let those through.
                if let Some(mapping) = self.ann.get_call_mapping(offset) {
                    let identity = mapping.len() == args.len()
                        && mapping.iter().enumerate().all(|(i, m)| *m == Some(i));
                    if !identity {
                        return unsupported("call with non-trivial arg mapping");
                    }
                }
                if self.extension_calls.contains_key(&offset) {
                    return unsupported("extension call");
                }
                // Only simple positional calls (no named/spread/defaults).
                let mut hargs = Vec::with_capacity(args.len());
                for a in args {
                    match a {
                        Arg::Positional(e) => hargs.push(self.lower_expr(e, scope)?),
                        _ => return unsupported("named/spread arg"),
                    }
                }
                // Method call: callee is a non-computed, non-optional `.name`
                // member (and not a `super.` call) → `CallMethod` with an IC.
                if let ExprKind::Member {
                    object,
                    property,
                    computed: false,
                    optional: false,
                } = &callee.kind
                {
                    if !matches!(object.kind, ExprKind::Super) {
                        if let ExprKind::Identifier { name } = &property.kind {
                            let recv = Box::new(self.lower_expr(object, scope)?);
                            return Ok(HirExpr::MethodCall {
                                recv,
                                name: name.clone(),
                                args: hargs,
                                ty: HirType::Dynamic,
                            });
                        }
                    }
                }
                // Statically-resolved self-recursion → `CallSelf` (see HIR doc).
                if self.is_self_call(callee, scope) {
                    return Ok(HirExpr::SelfCall {
                        args: hargs,
                        ty: HirType::Dynamic,
                    });
                }
                let callee = Box::new(self.lower_expr(callee, scope)?);
                Ok(HirExpr::Call {
                    callee,
                    args: hargs,
                    ty: HirType::Dynamic,
                })
            }
            ExprKind::Member {
                object,
                property,
                computed,
                optional,
            } => {
                if *optional {
                    // Optional chaining needs IsNull + jump; deferred.
                    return unsupported("optional member access");
                }
                if *computed {
                    // `object[index]` → GetIndex.
                    let object = Box::new(self.lower_expr(object, scope)?);
                    let index = Box::new(self.lower_expr(property, scope)?);
                    return Ok(HirExpr::Index {
                        object,
                        index,
                        ty: HirType::Dynamic,
                    });
                }
                // Non-computed `object.name` property read. Module-slot reads
                // (`LoadModuleSlot`) and extension members are desugared
                // differently by the legacy codegen → fall back for now.
                if self.ann.get_slot_idx(offset).is_some() {
                    return unsupported("module-slot member access");
                }
                if self.extension_members.contains_key(&offset) {
                    return unsupported("extension member access");
                }
                if matches!(object.kind, ExprKind::Super) {
                    return unsupported("super member access");
                }
                let name = match &property.kind {
                    ExprKind::Identifier { name } => name.clone(),
                    _ => return unsupported("non-identifier property"),
                };
                let object = Box::new(self.lower_expr(object, scope)?);
                Ok(HirExpr::Member {
                    object,
                    name,
                    ty: HirType::Dynamic,
                })
            }
            ExprKind::Logical { op, left, right } => {
                let lhs = Box::new(self.lower_expr(left, scope)?);
                let rhs = Box::new(self.lower_expr(right, scope)?);
                Ok(HirExpr::Logical {
                    op: logical_op(*op),
                    lhs,
                    rhs,
                })
            }
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                let test = Box::new(self.lower_expr(test, scope)?);
                let cons = Box::new(self.lower_expr(consequent, scope)?);
                let alt = Box::new(self.lower_expr(alternate, scope)?);
                Ok(HirExpr::Conditional { test, cons, alt })
            }
            ExprKind::Update {
                op,
                prefix,
                operand,
            } => {
                // Member/index update targets are §1.4; identifiers only here.
                let ExprKind::Identifier { name } = &operand.kind else {
                    return unsupported("update on non-identifier target");
                };
                let target = self.resolve(name, scope);
                Ok(HirExpr::Update {
                    target,
                    op: update_op(*op),
                    prefix: *prefix,
                })
            }
            ExprKind::Array { elements } => {
                // Simple array literal: plain element exprs, no spread/holes.
                let mut out = Vec::with_capacity(elements.len());
                for el in elements {
                    match el {
                        ArrayEl::Expr(e) => out.push(self.lower_expr(e, scope)?),
                        ArrayEl::Spread(_) => return unsupported("array spread element"),
                        ArrayEl::Hole => return unsupported("array hole"),
                    }
                }
                Ok(HirExpr::Array(out))
            }
            ExprKind::Object { properties } => {
                // Fixed-shape object: all-static keys, plain value props only.
                let mut keys = Vec::with_capacity(properties.len());
                let mut values = Vec::with_capacity(properties.len());
                for prop in properties {
                    match prop {
                        ObjectProp::Property {
                            key,
                            value,
                            computed: false,
                            ..
                        } => {
                            let k: Rc<str> = match key {
                                PropKey::Identifier(s) | PropKey::Str(s) => Rc::from(s.as_str()),
                                PropKey::Int(n) => Rc::from(n.to_string().as_str()),
                                PropKey::Computed(_) => return unsupported("computed object key"),
                            };
                            keys.push(k);
                            values.push(self.lower_expr(value, scope)?);
                        }
                        _ => return unsupported("object method/getter/setter/spread/computed"),
                    }
                }
                // Empty `{}` uses the legacy BuildObject path; defer.
                if keys.is_empty() {
                    return unsupported("empty object literal");
                }
                Ok(HirExpr::Object { keys, values })
            }
            ExprKind::Function {
                fn_id,
                params,
                body,
                is_async,
                is_generator,
                ..
            } => {
                // A named function expression can reference itself; resolving
                // that correctly needs a self-binding we don't model yet.
                if fn_id.is_some() {
                    return unsupported("named function expression");
                }
                let (func, upvalues) = self.lower_function_like(
                    Rc::from("<anon>"),
                    params,
                    *is_async,
                    *is_generator,
                    false,
                    false,
                    BodyRef::Block(body),
                    &[],
                    scope,
                )?;
                Ok(HirExpr::Closure {
                    func: Box::new(func),
                    upvalues,
                })
            }
            ExprKind::Arrow {
                params,
                body,
                is_async,
                ..
            } => {
                let body_ref = match body.as_ref() {
                    ArrowBody::Expr(e) => BodyRef::ExprBody(e),
                    ArrowBody::Block(s) => BodyRef::Block(s),
                };
                let (func, upvalues) = self.lower_function_like(
                    Rc::from("<arrow>"),
                    params,
                    *is_async,
                    false,
                    false,
                    false,
                    body_ref,
                    &[],
                    scope,
                )?;
                Ok(HirExpr::Closure {
                    func: Box::new(func),
                    upvalues,
                })
            }
            ExprKind::Match { subject, cases } => self.lower_match(subject, cases, scope),
            _ => unsupported("expression kind"),
        }
    }

    /// Whether `callee` is a statically-guaranteed reference to the enclosing
    /// function: a bare identifier equal to the current function's name, not
    /// shadowed by a local/param, and never reassigned in the module. Mirrors
    /// legacy `can_emit_self_call` (async/generator/rest/`this` cases are
    /// already excluded upstream because such functions fall back to legacy).
    fn is_self_call(&self, callee: &Expr, scope: &Scope) -> bool {
        let ExprKind::Identifier { name } = &callee.kind else {
            return false;
        };
        match &self.current_fn {
            Some(cur) if cur == name => {}
            _ => return false,
        }
        // Self-recursion is `CallSelf` (direct, no closure lookup) when the name
        // is not shadowed by a param/local of *this* function and is not
        // reassigned. Checking only the current frame — not capturing from a
        // parent — matches legacy `name_resolves_locally`, so a nested function
        // recurses via `CallSelf` instead of an upvalue to its own slot (which
        // would be a use-before-def the register allocator can't model).
        scope.resolve_in_current_frame(name).is_none() && !self.ann.is_reassigned_name(name)
    }

    fn resolve(&self, name: &Rc<str>, scope: &mut Scope) -> HirBinding {
        if let Some(b) = scope.resolve(name) {
            b
        } else {
            // Unresolved locally -> module global (covers builtins like `print`
            // too, which the VM resolves by name).
            HirBinding::Global(name.clone())
        }
    }
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
        _ => return unsupported("binary op"),
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
