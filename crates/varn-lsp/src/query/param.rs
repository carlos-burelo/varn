use crate::document::DocumentState;

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub type_str: String,

    pub is_type_param: bool,
}

pub fn param_at(state: &DocumentState, line: u32, col: u32) -> Option<ParamInfo> {
    if let Some(name) = state.type_param_at_pos(line, col) {
        return Some(ParamInfo {
            name,
            type_str: String::new(),
            is_type_param: true,
        });
    }
    if let Some((name, type_str)) = state.param_decl_at_pos(line, col) {
        return Some(ParamInfo {
            name,
            type_str,
            is_type_param: false,
        });
    }
    if let Some((name, type_str)) = state.param_usage_at_pos(line, col) {
        return Some(ParamInfo {
            name,
            type_str,
            is_type_param: false,
        });
    }
    None
}
