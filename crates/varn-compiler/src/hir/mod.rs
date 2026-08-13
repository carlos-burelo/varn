use rust_decimal::Decimal;
use std::rc::Rc;

pub mod ctor_summary;
pub mod dump;
pub mod inline;
pub mod lower;
pub mod module_locals;

/// Interned handle to a nested `HirType` in the module's [`TyTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyId(pub u32);

/// Interned handle to a class name in the module's [`TyTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirType {
    Int,
    Float,
    Bool,
    Str,
    Ref,
    Dynamic,
    /// `Array<T>`; element type behind a [`TyId`] to keep `HirType` `Copy`.
    Array(TyId),
    Map(TyId, TyId),
    Set(TyId),
    /// Instance of a source-declared class.
    Class(ClassId),
    /// `T?` — payload type plus null.
    Nullable(TyId),
}

/// Module-wide intern table resolving the [`TyId`]/[`ClassId`] handles that
/// structured [`HirType`]s carry. One per lowered module; shared onward so
/// SSA passes and emission can resolve nesting.
#[derive(Debug, Default)]
pub struct TyTable {
    entries: Vec<HirType>,
    dedup: rustc_hash::FxHashMap<HirType, u32>,
    class_names: Vec<Rc<str>>,
    class_dedup: rustc_hash::FxHashMap<Rc<str>, u32>,
}

impl TyTable {
    pub fn intern(&mut self, ty: HirType) -> TyId {
        if let Some(&i) = self.dedup.get(&ty) {
            return TyId(i);
        }
        let i = self.entries.len() as u32;
        self.entries.push(ty);
        self.dedup.insert(ty, i);
        TyId(i)
    }

    pub fn get(&self, id: TyId) -> HirType {
        self.entries[id.0 as usize]
    }

    pub fn class_id(&mut self, name: &Rc<str>) -> ClassId {
        if let Some(&i) = self.class_dedup.get(name) {
            return ClassId(i);
        }
        let i = self.class_names.len() as u32;
        self.class_names.push(name.clone());
        self.class_dedup.insert(name.clone(), i);
        ClassId(i)
    }

    pub fn class_name(&self, id: ClassId) -> &Rc<str> {
        &self.class_names[id.0 as usize]
    }

    /// Import a checker-side [`varn_core::CgTy`] projection. Kinds the
    /// backend has no representation for yet (char/decimal/bigint/fn)
    /// stay `Dynamic` — conservative, never wrong.
    pub fn from_cg(&mut self, cg: &varn_core::CgTy) -> HirType {
        use varn_core::CgTy;
        match cg {
            CgTy::Int => HirType::Int,
            CgTy::Float => HirType::Float,
            CgTy::Bool => HirType::Bool,
            CgTy::Str => HirType::Str,
            CgTy::Array(el) => {
                let e = self.from_cg(el);
                let id = self.intern(e);
                HirType::Array(id)
            }
            CgTy::Map(k, v) => {
                let kt = self.from_cg(k);
                let vt = self.from_cg(v);
                let ki = self.intern(kt);
                let vi = self.intern(vt);
                HirType::Map(ki, vi)
            }
            CgTy::Set(el) => {
                let e = self.from_cg(el);
                let id = self.intern(e);
                HirType::Set(id)
            }
            CgTy::Class(name) => HirType::Class(self.class_id(name)),
            CgTy::Nullable(inner) => {
                let t = self.from_cg(inner);
                if t == HirType::Dynamic {
                    HirType::Dynamic
                } else {
                    let id = self.intern(t);
                    HirType::Nullable(id)
                }
            }
            CgTy::Char | CgTy::Decimal | CgTy::BigInt | CgTy::Fn | CgTy::Dynamic => {
                HirType::Dynamic
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum HirBinding {
    Param(u32),
    Local(LocalId),
    Global(Rc<str>),
    Upvalue(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirUpvalueSrc {
    ParentLocal(LocalId),

    ParentParam(u32),

    ParentUpvalue(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTarget {
    Param(u32),
    Local(LocalId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,

    Ushr,

    Instanceof,

    In,
}

/// Result type of a `Binary` node whose `ty` field holds the OPERAND class.
/// Comparisons produce `Bool`; arithmetic keeps the operand class except
/// `int / int → float`, which is defined once in `varn_core::numeric`.
pub(crate) fn binary_result_ty(op: HirBinOp, operand_ty: HirType) -> HirType {
    use HirBinOp::*;
    match op {
        Eq | Ne | Lt | Le | Gt | Ge | Instanceof | In => HirType::Bool,
        Add | Sub | Mul | Div | Mod | Pow => {
            let k = match operand_ty {
                HirType::Int => varn_core::NumericOperand::Int,
                HirType::Float => varn_core::NumericOperand::Float,
                _ => return operand_ty,
            };
            let ast_op = match op {
                Div => varn_core::ast::operators::BinaryOp::Div,
                Add => varn_core::ast::operators::BinaryOp::Add,
                Sub => varn_core::ast::operators::BinaryOp::Sub,
                Mul => varn_core::ast::operators::BinaryOp::Mul,
                Mod => varn_core::ast::operators::BinaryOp::Mod,
                Pow => varn_core::ast::operators::BinaryOp::Pow,
                _ => unreachable!(),
            };
            match varn_core::binary_result_kind(ast_op, k) {
                varn_core::NumericOperand::Int => HirType::Int,
                varn_core::NumericOperand::Float => HirType::Float,
                varn_core::NumericOperand::Decimal => HirType::Dynamic,
            }
        }
        _ => operand_ty,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirUnOp {
    Neg,
    Not,
    BitNot,
    Typeof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirLogicalOp {
    And,
    Or,
    Nullish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirUpdateOp {
    Inc,
    Dec,
}

#[derive(Debug, Clone)]
pub enum HirTypeTest {
    IsNull,

    IsArray,

    TypeofEq(Rc<str>),

    Instanceof(Rc<str>),

    AlwaysFalse,
}

#[derive(Debug, Clone)]
pub enum HirExpr {
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bool(bool),
    Char(char),
    Decimal(Decimal),
    BigInt(i128),
    Regex {
        pattern: Rc<str>,
        flags: Rc<str>,
    },
    Null,

    NonNull(Box<HirExpr>),

    TryOp(Box<HirExpr>),

    TypeTest {
        value: Box<HirExpr>,
        kind: HirTypeTest,
    },

    Sequence(Vec<HirExpr>),

    Range {
        start: Box<HirExpr>,
        end: Box<HirExpr>,
        inclusive: bool,
    },

    Template(Vec<HirTemplatePart>),

    Assign {
        target: Box<HirAssignTarget>,
        value: Box<HirExpr>,
    },

    Var(HirBinding),
    Binary {
        op: HirBinOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
        ty: HirType,
    },
    Unary {
        op: HirUnOp,
        operand: Box<HirExpr>,
        ty: HirType,
    },

    Call {
        callee: Box<HirExpr>,
        args: Vec<HirExpr>,
        ty: HirType,
    },

    SelfCall {
        args: Vec<HirExpr>,
        ty: HirType,
    },

    Member {
        object: Box<HirExpr>,
        name: Rc<str>,
        ty: HirType,
    },

    GetFixedField {
        object: Box<HirExpr>,
        slot: u16,
        ty: HirType,
    },

    Index {
        object: Box<HirExpr>,
        index: Box<HirExpr>,
        ty: HirType,
        /// True when the checker proved the object is a statically-typed Array.
        is_array: bool,
    },

    MethodCall {
        recv: Box<HirExpr>,
        name: Rc<str>,
        args: Vec<HirExpr>,
        ty: HirType,
    },

    Logical {
        op: HirLogicalOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
    },

    Conditional {
        test: Box<HirExpr>,
        cons: Box<HirExpr>,
        alt: Box<HirExpr>,
    },

    Update {
        target: Box<HirAssignTarget>,
        op: HirUpdateOp,
        prefix: bool,
    },

    Array(Vec<HirArrayEl>),
    Tuple(Vec<HirArrayEl>),

    MemberMaybe {
        object: Box<HirExpr>,
        name: Rc<str>,
        ty: HirType,
    },

    ObjectRest {
        object: Box<HirExpr>,
        skip_keys: Vec<Rc<str>>,
    },

    OptionalChain {
        object: Box<HirExpr>,
        property: HirOptionalProperty,
    },

    Object {
        properties: Vec<HirObjectProp>,
    },
    Record {
        properties: Vec<HirObjectProp>,
    },

    Closure {
        func: Box<HirFunction>,
        upvalues: Vec<HirUpvalueSrc>,
    },

    This,

    Super,

    SuperCall {
        args: Vec<HirExpr>,
    },

    SuperMethodCall {
        name: Rc<str>,
        args: Vec<HirExpr>,
    },

    SuperMember {
        name: Rc<str>,
    },

    ExtensionCall {
        func: Rc<str>,
        recv: Box<HirExpr>,
        args: Vec<HirExpr>,
    },

    Class(Box<HirClass>),

    Enum(Box<HirEnum>),

    Match {
        subject: Box<HirExpr>,
        cases: Vec<HirMatchCase>,
    },

    Spread(Box<HirExpr>),

    Await(Box<HirExpr>),

    Spawn(Box<HirExpr>),

    Yield(Box<HirExpr>),

    TaggedTemplate {
        tag: Box<HirExpr>,
        template: Box<HirExpr>,
    },

    IntrinsicCall {
        object: Box<HirExpr>,
        args: Vec<HirExpr>,
        wire_byte: u8,
        ty: HirType,
    },

    /// Direct dispatch of a statically-typed core-type method by stable op-id.
    NativeMethodCall {
        object: Box<HirExpr>,
        args: Vec<HirExpr>,
        op_id: u64,
        ty: HirType,
    },

    ModuleSlot {
        object: Box<HirExpr>,
        slot: u16,
        ty: HirType,
    },
}

#[derive(Debug, Clone)]
pub enum HirArrayEl {
    Expr(HirExpr),
    Spread(HirExpr),
    Hole,
}

#[derive(Debug, Clone)]
pub enum HirObjectProp {
    Property {
        key: HirPropKey,
        value: HirExpr,
    },
    Method {
        key: HirPropKey,
        func: HirFunction,
        upvalues: Vec<HirUpvalueSrc>,
    },
    Spread(HirExpr),
}

#[derive(Debug, Clone)]
pub enum HirPropKey {
    Static(Rc<str>),
    Computed(HirExpr),
}

#[derive(Debug, Clone)]
pub enum HirOptionalProperty {
    Member(Rc<str>),
    Index(Box<HirExpr>),
    ModuleSlot(u16),
    Extension(Rc<str>),
    Call(Vec<HirExpr>),
    MethodCall(Rc<str>, Vec<HirExpr>),
    ExtensionCall(Rc<str>, Vec<HirExpr>),
}

#[derive(Debug, Clone)]
pub enum HirTemplatePart {
    Str(Rc<str>),
    Expr(HirExpr),
}

#[derive(Debug, Clone)]
pub enum HirAssignTarget {
    Var(HirBinding),

    Member {
        object: HirExpr,
        name: Rc<str>,
    },

    SetFixedField {
        object: HirExpr,
        slot: u16,
    },

    Index {
        object: HirExpr,
        index: HirExpr,
        /// True when the checker proved the object is a statically-typed Array.
        is_array: bool,
    },

    ModuleSlot {
        slot: u16,
    },

    SuperMember {
        name: Rc<str>,
    },

    SuperIndex {
        index: HirExpr,
    },
}

#[derive(Debug, Clone)]
pub struct HirEnumVariant {
    pub name: Rc<str>,
    pub tag: i64,
    pub meta: Rc<str>,
    pub const_args: Vec<HirExpr>,
}

#[derive(Debug, Clone)]
pub struct HirEnum {
    pub name: Rc<str>,
    pub variants: Vec<HirEnumVariant>,
    pub fields: Vec<Rc<str>>,
    pub static_fields: Vec<(Rc<str>, Option<HirExpr>)>,
    pub ctor: HirMethod,
    pub methods: Vec<HirMethod>,
    pub static_methods: Vec<HirMethod>,
    pub getters: Vec<HirAccessor>,
    pub setters: Vec<HirAccessor>,
    pub static_blocks: Vec<HirMethod>,
}

#[derive(Debug, Clone)]
pub struct HirMatchCase {
    pub test: HirCaseTest,

    pub guard: Option<HirExpr>,
    pub body: Vec<HirStmt>,

    pub result: Option<HirExpr>,
}

#[derive(Debug, Clone)]
pub enum HirCaseTest {
    Wildcard,

    Literal(HirExpr),

    Bind(LocalId),

    EnumVariant {
        name: Rc<str>,
        binds: Vec<Option<LocalId>>,
    },

    Record {
        fields: Vec<(Rc<str>, Option<LocalId>)>,
    },
}

#[derive(Debug, Clone)]
pub struct HirMethod {
    pub key: Rc<str>,
    pub func: HirFunction,
    pub upvalues: Vec<HirUpvalueSrc>,
    pub decorators: Vec<HirExpr>,
    pub is_private: bool,
}

#[derive(Debug, Clone)]
pub struct HirAccessor {
    pub key: Rc<str>,
    pub func: HirFunction,
    pub upvalues: Vec<HirUpvalueSrc>,
    pub is_static: bool,
}

#[derive(Debug, Clone)]
pub struct HirClass {
    pub name: Rc<str>,

    pub super_class: Option<HirExpr>,

    pub fields: Vec<Rc<str>>,

    pub static_fields: Vec<(Rc<str>, Option<HirExpr>)>,

    pub ctor: HirMethod,

    pub methods: Vec<HirMethod>,

    pub static_methods: Vec<HirMethod>,

    pub getters: Vec<HirAccessor>,
    pub setters: Vec<HirAccessor>,

    pub static_blocks: Vec<HirMethod>,
    pub decorators: Vec<HirExpr>,
}

#[derive(Debug, Clone)]
pub enum HirStmt {
    Expr(HirExpr),

    Let {
        local: LocalId,
        value: HirExpr,
        ty: HirType,
    },

    Assign {
        target: HirBinding,
        value: HirExpr,
    },

    SetMember {
        object: HirExpr,
        name: Rc<str>,
        value: HirExpr,
    },

    /// Property store the checker resolved to a fixed slot (declared class
    /// field on a statically-typed receiver) — compiles to SetFixedField.
    SetFixedField {
        object: HirExpr,
        slot: u16,
        value: HirExpr,
    },

    SetIndex {
        object: HirExpr,
        index: HirExpr,
        value: HirExpr,
        /// True when the checker proved the object is a statically-typed Array.
        is_array: bool,
    },
    Return(Option<HirExpr>),
    If {
        test: HirExpr,
        then_body: Vec<HirStmt>,
        else_body: Vec<HirStmt>,
    },

    While {
        test: HirExpr,
        body: Vec<HirStmt>,
    },

    ForClassic {
        test: HirExpr,
        update: Vec<HirStmt>,
        body: Vec<HirStmt>,
    },

    ForOf {
        var: LocalId,
        iterable: HirExpr,
        body: Vec<HirStmt>,
        is_await: bool,
    },

    ForIn {
        var: LocalId,
        object: HirExpr,
        body: Vec<HirStmt>,
    },

    DoWhile {
        body: Vec<HirStmt>,
        test: HirExpr,
    },

    Switch {
        disc: HirExpr,
        cases: Vec<HirSwitchCase>,
    },
    Break,
    Continue,

    Throw(HirExpr),

    Try {
        block: Vec<HirStmt>,
        catch: Option<HirCatch>,
        finally: Option<Vec<HirStmt>>,
    },

    CloseUpvalues(Vec<CaptureTarget>),

    Import {
        source: Rc<str>,
        is_type: bool,
        specs: Vec<HirImportSpec>,
    },

    StoreExport {
        name: Rc<str>,
        slot: u16,
    },

    ExportNamed {
        specifiers: Vec<HirExportSpec>,
        source: Option<Rc<str>>,
    },

    ExportAll {
        source: Rc<str>,
        alias: Option<Rc<str>>,
        slot: Option<u16>,
    },

    ExportDefaultExpr {
        value: HirExpr,
        slot: Option<u16>,
    },

    Dispose {
        target: LocalId,
        is_await: bool,
    },
}

#[derive(Debug, Clone)]
pub struct HirExportSpec {
    pub binding: HirBinding,
    pub local: Rc<str>,
    pub exported: Rc<str>,
    pub local_slot: Option<u16>,
    pub exported_slot: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct HirSwitchCase {
    pub test: Option<HirExpr>,
    pub body: Vec<HirStmt>,
}

#[derive(Debug, Clone)]
pub struct HirCatch {
    pub param: Option<LocalId>,
    pub body: Vec<HirStmt>,
}

#[derive(Debug, Clone)]
pub struct HirImportSpec {
    pub local: Rc<str>,
    pub kind: HirImportKind,

    pub slot: Option<u16>,
}

#[derive(Debug, Clone)]
pub enum HirImportKind {
    Default,

    Named(Rc<str>),

    Namespace,
}

#[derive(Debug, Clone)]
pub struct HirParam {
    pub name: Rc<str>,
    pub ty: HirType,

    pub default: Option<HirExpr>,
}

#[derive(Debug, Clone)]
pub struct HirFunction {
    pub name: Rc<str>,
    pub params: Vec<HirParam>,
    pub locals: u32,
    pub body: Vec<HirStmt>,
    pub return_ty: HirType,

    pub upvalue_count: u32,

    pub has_this: bool,

    pub has_rest: bool,
    pub is_async: bool,
    pub is_generator: bool,
}

#[derive(Debug, Clone)]
pub struct HirModule {
    pub top_level: HirFunction,
    pub functions: Vec<HirFunction>,
    /// The module's own path, exactly as `global_binding` spells it when it
    /// qualifies a locally-declared global (`<source_file>::<name>`). Passes
    /// that need to match a call site's global name against a function in
    /// `functions` must rebuild the qualified form from this; the bare
    /// `HirFunction::name` never appears at a call site.
    pub source_file: Rc<str>,
    /// Resolves the `TyId`/`ClassId` handles inside this module's
    /// structured `HirType`s. Frozen after lowering.
    pub ty_table: Rc<TyTable>,
    /// Qualified names of module-private top-level `let`/`const` — the
    /// promotion candidates `module_locals` filters down. See the field of
    /// the same name on `Lowerer` for why they are collected there.
    pub top_level_lets: Vec<Rc<str>>,
}
