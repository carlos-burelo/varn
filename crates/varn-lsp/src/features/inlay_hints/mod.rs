pub mod param_hints;
pub mod type_hints;

use crate::document::DocumentState;
use tower_lsp::lsp_types::InlayHint;

pub fn build_inlay_hints(state: &DocumentState) -> Vec<InlayHint> {
    let mut hints = type_hints::build_type_hints(state);
    hints.extend(param_hints::build_parameter_hints(state));
    hints
}
