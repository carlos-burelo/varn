use super::stmts::collect_stmt;
use varn_core::ast::{AstArena, AstId, ExprId, Program};

#[derive(Clone, Copy, Debug)]
pub struct SpatialEntry {
    pub start: u32,
    pub end: u32,
    pub expr: ExprId,
}

#[derive(Clone, Debug, Default)]
pub struct SpatialIndex {
    entries: Vec<SpatialEntry>,
}

impl SpatialIndex {
    pub fn build(program: &Program, a: &AstArena) -> Self {
        let initial_cap = program.body.len().saturating_mul(4).clamp(16, 512);
        let mut entries = Vec::with_capacity(initial_cap);
        for stmt in &program.body {
            collect_stmt(a, stmt, &mut entries);
        }
        entries.shrink_to_fit();

        entries.sort_by(|a, b| {
            a.start
                .cmp(&b.start)
                .then_with(|| (b.end.saturating_sub(b.start)).cmp(&a.end.saturating_sub(a.start)))
        });
        Self { entries }
    }

    pub fn innermost_at(&self, offset: u32) -> Option<AstId> {
        if self.entries.is_empty() {
            return None;
        }

        let upper = match self.entries.binary_search_by(|e| e.start.cmp(&offset)) {
            Ok(idx) => {
                let mut i = idx;
                while i + 1 < self.entries.len() && self.entries[i + 1].start == offset {
                    i += 1;
                }
                i + 1
            }
            Err(idx) => idx,
        };

        let mut best: Option<(u32, AstId)> = None;
        for entry in &self.entries[..upper] {
            if entry.start <= offset && offset <= entry.end {
                let span_len = entry.end.saturating_sub(entry.start);
                match best {
                    None => best = Some((span_len, entry.expr.index())),
                    Some((best_len, _)) if span_len < best_len => {
                        best = Some((span_len, entry.expr.index()));
                    }
                    Some(_) => {}
                }
            }
        }

        best.map(|(_, id)| id)
    }

    pub fn exprs(&self) -> impl Iterator<Item = ExprId> + '_ {
        self.entries.iter().map(|e| e.expr)
    }
}
