use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use rustc_hash::FxHashMap;
use varn_core::ast::ExprId;
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn narrow_logical_and(
        &mut self,
        rec: &mut Recorder,
        left: ExprId,
        right: ExprId,
        bind: &BindResult,
        is_true_branch: bool,
        out: &mut Vec<(SymbolId, Type)>,
    ) {
        if is_true_branch {
            out.extend(self.extract_narrowings(rec, left, bind, true));
            out.extend(self.extract_narrowings(rec, right, bind, true));
        } else {
            let left_n = self.extract_narrowings(rec, left, bind, false);
            let right_n = self.extract_narrowings(rec, right, bind, false);
            out.extend(self.merge_alternative_narrowings(left_n, right_n));
        }
    }

    pub(crate) fn narrow_logical_or(
        &mut self,
        rec: &mut Recorder,
        left: ExprId,
        right: ExprId,
        bind: &BindResult,
        is_true_branch: bool,
        out: &mut Vec<(SymbolId, Type)>,
    ) {
        if is_true_branch {
            let left_n = self.extract_narrowings(rec, left, bind, true);
            let right_n = self.extract_narrowings(rec, right, bind, true);
            out.extend(self.merge_alternative_narrowings(left_n, right_n));
        } else {
            out.extend(self.extract_narrowings(rec, left, bind, false));
            out.extend(self.extract_narrowings(rec, right, bind, false));
        }
    }

    pub(crate) fn merge_alternative_narrowings(
        &mut self,
        left: Vec<(SymbolId, Type)>,
        right: Vec<(SymbolId, Type)>,
    ) -> Vec<(SymbolId, Type)> {
        let first_per_symbol = |entries: Vec<(SymbolId, Type)>| {
            let mut map: FxHashMap<SymbolId, Type> = FxHashMap::default();
            for (id, ty) in entries {
                map.entry(id).or_insert(ty);
            }
            map
        };
        let left = first_per_symbol(left);
        let right = first_per_symbol(right);
        let mut merged = Vec::new();
        for (id, left_ty) in left {
            if let Some(&right_ty) = right.get(&id) {
                merged.push((
                    id,
                    Type::union(
                        vec![left_ty, right_ty],
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    ),
                ));
            }
        }
        merged
    }
}
