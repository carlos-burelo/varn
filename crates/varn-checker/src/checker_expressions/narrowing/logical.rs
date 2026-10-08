use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use rustc_hash::FxHashMap;
use varn_core::ast::ExprId;
use varn_core::TypeKind;
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
            out.extend(self.merge_narrowings(left_n, right_n, false));
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
            out.extend(self.merge_narrowings(left_n, right_n, true));
        } else {
            out.extend(self.extract_narrowings(rec, left, bind, false));
            out.extend(self.extract_narrowings(rec, right, bind, false));
        }
    }

    pub(crate) fn merge_narrowings(
        &mut self,
        left: Vec<(SymbolId, Type)>,
        right: Vec<(SymbolId, Type)>,
        is_union: bool,
    ) -> Vec<(SymbolId, Type)> {
        let mut map: FxHashMap<SymbolId, Vec<Type>> = FxHashMap::default();
        for (id, ty) in left {
            map.entry(id).or_default().push(ty);
        }
        for (id, ty) in right {
            map.entry(id).or_default().push(ty);
        }

        let mut merged = Vec::new();
        for (id, types) in map {
            if types.len() == 1 {
                if !is_union {
                    merged.push((id, types[0]));
                }
            } else if is_union {
                merged.push((
                    id,
                    Type::union(types, &mut *std::sync::Arc::make_mut(&mut self.ty_table)),
                ));
            } else {
                let ids: Vec<varn_sem::types::CheckerTyId> = types.iter().map(|t| t.0).collect();
                let list = std::sync::Arc::make_mut(&mut self.ty_table).intern_list(&ids);
                let interned = std::sync::Arc::make_mut(&mut self.ty_table)
                    .intern(TypeKind::Intersection(list));
                merged.push((id, Type::resolved(interned)));
            }
        }
        merged
    }
}
