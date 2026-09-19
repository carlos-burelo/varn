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

/// On-disk twin of [`Symbol`]: every `Atom`-bearing field here resolved to
/// its owned text.
///
/// `Symbol` itself keeps `Atom` in memory (cheap comparisons, cheap clones
/// within one compilation session), but `Atom` is a per-`AtomInterner` index
/// with no meaning outside the table that minted it — see the `#[serde(skip)]`
/// notes on `Symbol`. The module-interface cache
/// (`module_resolver::cache::CachedModule`) is read back by a *different*
/// process, with an empty `AtomInterner`, so a raw `Atom` round-tripped
/// through `postcard` would silently resolve to whatever text happens to sit
/// at that index in the new session (usually index 0) instead of failing —
/// wrong symbol names with no error. This type is the serializable form: text
/// crosses the disk boundary, `Atom` never does.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct CacheableSymbol {
    pub kind: SymbolKind,
    pub name: String,
    pub ty: Option<Type>,
    pub line: u32,
    pub col: u32,
    pub has_explicit_type: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub doc: Option<String>,
    pub type_params: Vec<String>,
    pub type_param_constraints: Vec<Option<Type>>,
    pub offset: u32,
    // `full_range` is not carried here: `varn_core::SourceRange` has no
    // serde impl (deliberately — see its own `#[serde(skip)]` on `Symbol`),
    // unrelated to the `Atom` round-trip this type exists for. It defaults
    // on load exactly as it already does when `Symbol` is (de)serialized
    // directly.
    pub origin_module: Option<String>,
    pub re_export_path: Vec<String>,
    pub original_name: Option<String>,
    pub slot_idx: Option<usize>,
    pub intrinsic_wire: Option<u8>,
}

impl Symbol {
    /// Resolve every `Atom` this symbol carries against `interner` (the
    /// `AtomInterner` of the session that produced it) into an owned,
    /// process-independent form fit to write to disk.
    ///
    /// `alias_node` is dropped, same as it already is under
    /// `#[serde(skip)]`: it is `Box<TypeNode>` from the live AST, never
    /// reconstructed from cache today (see its skip note above), so there is
    /// nothing sound to serialize it as.
    pub(crate) fn to_cacheable(&self, interner: &varn_core::AtomInterner) -> CacheableSymbol {
        CacheableSymbol {
            kind: self.kind,
            name: interner.resolve(self.name).to_string(),
            ty: self.ty.clone(),
            line: self.line,
            col: self.col,
            has_explicit_type: self.has_explicit_type,
            is_async: self.is_async,
            is_generator: self.is_generator,
            doc: self.doc.map(|a| interner.resolve(a).to_string()),
            type_params: self
                .type_params
                .iter()
                .map(|a| interner.resolve(*a).to_string())
                .collect(),
            type_param_constraints: self.type_param_constraints.clone(),
            offset: self.offset,
            origin_module: self.origin_module.map(|a| interner.resolve(a).to_string()),
            re_export_path: self
                .re_export_path
                .iter()
                .map(|a| interner.resolve(*a).to_string())
                .collect(),
            original_name: self
                .original_name
                .map(|a| interner.resolve(a).to_string()),
            slot_idx: self.slot_idx,
            intrinsic_wire: self.intrinsic_wire,
        }
    }

    /// Re-intern every text field of `c` against `interner` (the
    /// `AtomInterner` of the session doing the loading), producing a `Symbol`
    /// whose `Atom`s are valid for *this* compilation.
    pub(crate) fn from_cacheable(c: CacheableSymbol, interner: &mut varn_core::AtomInterner) -> Symbol {
        Symbol {
            kind: c.kind,
            name: interner.intern(&c.name),
            ty: c.ty,
            line: c.line,
            col: c.col,
            has_explicit_type: c.has_explicit_type,
            is_async: c.is_async,
            is_generator: c.is_generator,
            doc: c.doc.map(|s| interner.intern(&s)),
            type_params: c.type_params.iter().map(|s| interner.intern(s)).collect(),
            type_param_constraints: c.type_param_constraints,
            offset: c.offset,
            full_range: SourceRange::default(),
            origin_module: c.origin_module.map(|s| interner.intern(&s)),
            re_export_path: c.re_export_path.iter().map(|s| interner.intern(s)).collect(),
            original_name: c.original_name.map(|s| interner.intern(&s)),
            alias_node: None,
            slot_idx: c.slot_idx,
            intrinsic_wire: c.intrinsic_wire,
        }
    }

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

    pub fn all_mut(&mut self) -> &mut [Symbol] {
        &mut self.symbols
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
