use crate::document::DocumentState;

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub type_str: String,

    pub is_type_param: bool,
}




pub fn param_at(state: &DocumentState, line: u32, col: u32) -> Option<ParamInfo> {
    let name = state.type_param_at_pos(line, col)?;
    Some(ParamInfo {
        name,
        type_str: String::new(),
        is_type_param: true,
    })
}
