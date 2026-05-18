pub mod member;
pub mod param;
pub mod symbol;
pub mod token;

use crate::document::{ChainResult, DocumentState};

pub use crate::document::import::{
    import_path_at, named_import_module_at, named_imported_names_at, uri_to_path,
};
pub use member::{member_at, members_of, method_info_at};
pub use param::{param_at, ParamInfo};
pub use symbol::{symbol_at, symbol_at_line, symbols_named};
pub use token::{next_token, prev_token, token_at, token_index_at};

pub fn resolve_chain<'a>(state: &'a DocumentState, line: u32, col: u32) -> Option<ChainResult<'a>> {
    state.resolve_chain_at(line, col)
}
