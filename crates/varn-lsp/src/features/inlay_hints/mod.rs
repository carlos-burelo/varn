pub mod param_hints;
pub mod type_hints;

use tower_lsp::lsp_types::InlayHint;
use crate::document::DocumentState;

pub fn build_inlay_hints(state: &DocumentState) -> Vec<InlayHint> {
    let mut hints = type_hints::build_type_hints(state);
    hints.extend(param_hints::build_parameter_hints(state));
    hints
}
