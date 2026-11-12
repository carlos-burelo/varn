use tower_lsp_f::lsp_types::*;

pub(super) fn encode_tokens(raw: &[u32]) -> Vec<SemanticToken> {
    raw.chunks_exact(5)
        .map(|c| SemanticToken {
            delta_line: c[0],
            delta_start: c[1],
            length: c[2],
            token_type: c[3],
            token_modifiers_bitset: c[4],
        })
        .collect()
}

pub(super) fn data_id(data: &[SemanticToken]) -> String {
    use rustc_hash::FxHasher;
    use std::hash::{Hash, Hasher};
    let mut h = FxHasher::default();
    h.write_usize(data.len());
    for t in data.iter().take(64) {
        t.delta_line.hash(&mut h);
        t.delta_start.hash(&mut h);
        t.length.hash(&mut h);
        t.token_type.hash(&mut h);
    }
    format!("{:x}", h.finish())
}

pub(super) fn filter_tokens_to_range(raw: &[u32], range: Range) -> Vec<SemanticToken> {
    let mut absolute: Vec<(u32, u32, SemanticToken)> = Vec::new();
    let mut line = 0u32;
    let mut col = 0u32;
    for c in raw.chunks_exact(5) {
        line += c[0];
        col = if c[0] == 0 { col + c[1] } else { c[1] };
        if line < range.start.line || line > range.end.line {
            continue;
        }
        absolute.push((
            line,
            col,
            SemanticToken {
                delta_line: 0,
                delta_start: 0,
                length: c[2],
                token_type: c[3],
                token_modifiers_bitset: c[4],
            },
        ));
    }
    let mut result = Vec::with_capacity(absolute.len());
    let mut rel_line = 0u32;
    let mut rel_col = 0u32;
    for (l, co, mut t) in absolute {
        t.delta_line = l - rel_line;
        t.delta_start = if l == rel_line { co - rel_col } else { co };
        rel_line = l;
        rel_col = co;
        result.push(t);
    }
    result
}
