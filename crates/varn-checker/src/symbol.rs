use crate::types::Type;
pub type SymbolId = usize;
use varn_core::ast::TypeNode;
use varn_core::Atom;
use varn_core::SourceRange;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SymbolKind {
    Var,
    Let,
    Const,
    Function,
    Class,
    Interface,
    TypeAlias,
    Enum,
    Parameter,
    Property,
    Method,
    TypeParameter,
    Namespace,
    Struct,
    Extension,
    EnumMember,
}

impl SymbolKind {
    pub fn label(self) -> &'static str {
        match self {
            SymbolKind::Var => "var  ",
            SymbolKind::Let => "let  ",
            SymbolKind::Const => "const",
            SymbolKind::Function => "fn   ",
            SymbolKind::Class => "class  ",
            SymbolKind::Interface => "interface",
            SymbolKind::TypeAlias => "type ",
            SymbolKind::Enum => "enum ",
            SymbolKind::Parameter => "param",
            SymbolKind::Property => "prop ",
            SymbolKind::Method => "method ",
            SymbolKind::TypeParameter => "type_param",
            SymbolKind::Namespace => "namespace   ",
            SymbolKind::Struct => "struct",
            SymbolKind::Extension => "extension  ",
            SymbolKind::EnumMember => "enum_member",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Symbol {
    pub kind: SymbolKind,
    // `Atom` is a per-parse interned index and does not (and should not)
    // derive `serde::{Serialize, Deserialize}` — see
    // `binder::types::TypeMembers::objects` for the established rationale.
    // Every `Atom`-typed field below is skipped for the same reason; a
    // reloaded (cached) `Symbol` needs its names re-resolved against a live
    // interner by its caller, same as `BindResult::interner`.
    #[serde(skip)]
    pub name: Atom,
    pub ty: Option<Type>,
    pub line: u32,
    pub col: u32,
    pub has_explicit_type: bool,
    pub is_async: bool,
    pub is_generator: bool,
    #[serde(skip)]
    pub doc: Option<Atom>,
    #[serde(skip)]
    pub type_params: Vec<Atom>,
    pub type_param_constraints: Vec<Option<Type>>,
    pub offset: u32,
    #[serde(skip)]
    pub full_range: varn_core::SourceRange,
    #[serde(skip)]
    pub origin_module: Option<Atom>,
    #[serde(skip)]
    pub re_export_path: Vec<Atom>,
    #[serde(skip)]
    pub original_name: Option<Atom>,
    #[serde(skip)]
    pub alias_node: Option<Box<TypeNode>>,
    pub slot_idx: Option<usize>,
    /// Intrinsic wire byte when this symbol is a free-function intrinsic import
    /// (e.g. `abs` from `std:math`). Set at import-bind time where the module
    /// specifier is known; lets bare calls lower to `OpCode::Intrinsic`.
    #[serde(default)]
    pub intrinsic_wire: Option<u8>,
}

impl Symbol {
    pub fn new(kind: SymbolKind, name: Atom, line: u32) -> Self {
        Self {
            kind,
            name,
            ty: None,
            line,
            col: 0,
            has_explicit_type: false,
            is_async: false,
            is_generator: false,
            doc: None,
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            offset: 0,
            full_range: SourceRange::default(),
            origin_module: None,
            re_export_path: Vec::new(),
            original_name: None,
            alias_node: None,
            slot_idx: None,
            intrinsic_wire: None,
        }
    }

    pub fn with_type(mut self, ty: Type) -> Self {
        self.ty = Some(ty);
        self
    }
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SymbolArena {
    symbols: Vec<Symbol>,
}

impl SymbolArena {
    pub fn push(&mut self, symbol: Symbol) -> SymbolId {
        let id = self.symbols.len();
        self.symbols.push(symbol);
        id
    }

    pub fn get(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id]
    }

    pub fn get_mut(&mut self, id: SymbolId) -> &mut Symbol {
        &mut self.symbols[id]
    }

    pub fn all(&self) -> &[Symbol] {
        &self.symbols
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    pub fn find_id_by_name_and_line(&self, name: Atom, line: u32) -> Option<SymbolId> {
        self.symbols
            .iter()
            .position(|s| s.name == name && s.line == line)
    }
}
