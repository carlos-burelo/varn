use crate::types::Type;
use std::rc::Rc;
pub type SymbolId = usize;
use varn_core::ast::TypeNode;
use varn_core::SourceRange;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: Rc<str>,
    pub ty: Option<Type>,
    pub line: u32,
    pub col: u32,
    pub has_explicit_type: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub doc: Option<Rc<str>>,
    pub type_params: Vec<Rc<str>>,
    pub type_param_constraints: Vec<Option<Type>>,
    pub offset: u32,
    pub full_range: varn_core::SourceRange,
    pub origin_module: Option<Rc<str>>,
    pub re_export_path: Vec<Rc<str>>,
    pub original_name: Option<Rc<str>>,
    pub alias_node: Option<Box<TypeNode>>,
    pub slot_idx: Option<usize>,
}

impl Symbol {
    pub fn new(kind: SymbolKind, name: Rc<str>, line: u32) -> Self {
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
        }
    }

    pub fn with_type(mut self, ty: Type) -> Self {
        self.ty = Some(ty);
        self
    }
}

#[derive(Clone, Default)]
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

    pub fn find_id_by_name_and_line(&self, name: &str, line: u32) -> Option<SymbolId> {
        self.symbols
            .iter()
            .position(|s| s.name.as_ref() == name && s.line == line)
    }
}
