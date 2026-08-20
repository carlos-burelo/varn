use std::rc::Rc;
use varn_core::source::SourceRange;
use crate::types::Type;

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
}

