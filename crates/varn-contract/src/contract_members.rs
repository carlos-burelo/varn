use crate::varn_contract::Mapped;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Method,
    Getter,
    StaticMethod,
    StaticGetter,
    Constructor,
    Property,

    Function,
}

pub(crate) struct ParamInfo {
    pub(crate) mapped: Mapped,
    pub(crate) is_rest: bool,
}

pub(crate) struct Member {
    pub(crate) symbol: String,
    pub(crate) kind: Kind,
    pub(crate) params: Vec<ParamInfo>,
    pub(crate) ret: Mapped,

    pub(crate) fallible: bool,
}
