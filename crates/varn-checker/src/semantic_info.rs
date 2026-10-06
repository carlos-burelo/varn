use crate::types::Type;
use std::sync::Arc;
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
    pub member_name: Arc<str>,
    pub member_kind: ResolvedMemberKind,
    pub member_ty: Type,
    pub origin_module: Option<Arc<str>>,
    pub def_range: Option<SourceRange>,
    pub doc: Option<Arc<str>>,
}





#[derive(Clone, Debug, Default)]
pub struct MatchGap {
    pub missing: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CallParamInfo {
    pub name: Option<Arc<str>>,
    pub ty: Type,
    pub optional: bool,
    pub is_rest: bool,
}

#[derive(Clone, Debug)]
pub struct CallResolution {
    pub callee_name: Option<Arc<str>>,
    pub params: Vec<CallParamInfo>,
    pub return_ty: Type,
    pub arg_to_param_map: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct ResolvedMemberSummary {
    pub name: Arc<str>,
    pub ty: Type,
    pub kind: ResolvedMemberKind,
    pub is_static: bool,
    pub optional: bool,
    pub readonly: bool,
    
    
    
    
    
    
    
    pub def_line: Option<u32>,
    pub def_col: u32,
    pub is_async: bool,
    pub is_generator: bool,
}
