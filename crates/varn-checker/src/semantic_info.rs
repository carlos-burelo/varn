use crate::types::Type;
use std::rc::Rc;
use varn_core::source::SourceRange;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedMemberKind {
    Method,
    Property,
    Getter,
    Setter,
    EnumMember,
    StaticMethod,
    StaticProperty,
    ExtensionMethod,
    ExtensionProperty,
    /// A type declared *inside* another — a class in a namespace, an enum in a
    /// class. Distinct from `Property` because an editor renders it as the
    /// declaration it is, not as a field that happens to hold a type.
    ///
    /// These used to collapse to `Property` here and be recovered from a
    /// parallel `MemberKind` in the language server, which is why that enum
    /// outlived the table it belonged to.
    NestedType(NestedTypeKind),
    Constructor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NestedTypeKind {
    Class,
    Interface,
    Namespace,
    Enum,
    Struct,
}

impl ResolvedMemberKind {
    pub fn label(&self) -> &'static str {
        match self {
            ResolvedMemberKind::Method => "method",
            ResolvedMemberKind::Property => "property",
            ResolvedMemberKind::Getter => "getter",
            ResolvedMemberKind::Setter => "setter",
            ResolvedMemberKind::EnumMember => "enum member",
            ResolvedMemberKind::StaticMethod => "static method",
            ResolvedMemberKind::StaticProperty => "static property",
            ResolvedMemberKind::ExtensionMethod => "extension method",
            ResolvedMemberKind::ExtensionProperty => "extension property",
            ResolvedMemberKind::Constructor => "constructor",
            ResolvedMemberKind::NestedType(k) => k.label(),
        }
    }
}

impl NestedTypeKind {
    pub fn label(&self) -> &'static str {
        match self {
            NestedTypeKind::Class => "class",
            NestedTypeKind::Interface => "interface",
            NestedTypeKind::Namespace => "namespace",
            NestedTypeKind::Enum => "enum",
            NestedTypeKind::Struct => "struct",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MemberResolution {
    pub receiver_ty: Type,
    pub member_name: Rc<str>,
    pub member_kind: ResolvedMemberKind,
    pub member_ty: Type,
    pub origin_module: Option<Rc<str>>,
    pub def_range: Option<SourceRange>,
    pub doc: Option<Rc<str>>,
}

#[derive(Clone, Debug)]
pub struct CallParamInfo {
    pub name: Option<Rc<str>>,
    pub ty: Type,
    pub optional: bool,
    pub is_rest: bool,
}

#[derive(Clone, Debug)]
pub struct CallResolution {
    pub callee_name: Option<Rc<str>>,
    pub params: Vec<CallParamInfo>,
    pub return_ty: Type,
    pub arg_to_param_map: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct ResolvedMemberSummary {
    pub name: Rc<str>,
    pub ty: Type,
    pub kind: ResolvedMemberKind,
    pub is_static: bool,
    pub optional: bool,
    pub readonly: bool,
    /// Where the member is declared, 1-based, when it is declared in source.
    ///
    /// `None` for a member that has no source of its own: one read out of a
    /// precompiled interface blob, or synthesised (a tuple index, an intrinsic
    /// property). An editor needs this to offer "go to" and to build an
    /// outline, so a summary without it forces the caller to keep a parallel
    /// table that has it — which is exactly what this replaces.
    pub def_line: Option<u32>,
    pub def_col: u32,
    pub is_async: bool,
    pub is_generator: bool,
}
