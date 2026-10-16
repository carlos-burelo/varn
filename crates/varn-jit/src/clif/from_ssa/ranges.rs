use rustc_hash::{FxHashMap, FxHashSet};
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{SsaBinOp, SsaOp, SsaProto, SsaTerm};

const I64_LO: i128 = i64::MIN as i128;
const I64_HI: i128 = i64::MAX as i128;

#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Jump,
    Then,
    Else,
}

pub(super) struct VersionPlan {
    pub header: usize,
    pub preheader: usize,
    pub guards: Vec<(u32, i128)>,
    pub elide: FxHashSet<u32>,
}

fn def_map(ssa: &SsaProto) -> Vec<Option<&SsaOp>> {
    let mut def = vec![None; ssa.values.len()];
    for blk in &ssa.blocks {
        for inst in &blk.insts {
            if let Some(d) = inst.dest {
                def[d as usize] = Some(&inst.op);
            }
        }
    }
    def
}

fn def_block(ssa: &SsaProto) -> Vec<Option<usize>> {
    let mut def = vec![None; ssa.values.len()];
    for (b, blk) in ssa.blocks.iter().enumerate() {
        for &p in &blk.params {
            def[p as usize] = Some(b);
        }
        for inst in &blk.insts {
            if let Some(d) = inst.dest {
                def[d as usize] = Some(b);
            }
        }
    }
    def
}

fn add_range(a: (i128, i128), b: (i128, i128)) -> Option<(i128, i128)> {
    Some((a.0.checked_add(b.0)?, a.1.checked_add(b.1)?))
}

fn sub_range(a: (i128, i128), b: (i128, i128)) -> Option<(i128, i128)> {
    Some((a.0.checked_sub(b.1)?, a.1.checked_sub(b.0)?))
}

fn mul_range(a: (i128, i128), b: (i128, i128)) -> Option<(i128, i128)> {
    let ps = [
        a.0.checked_mul(b.0)?,
        a.0.checked_mul(b.1)?,
        a.1.checked_mul(b.0)?,
        a.1.checked_mul(b.1)?,
    ];
    Some((*ps.iter().min().unwrap(), *ps.iter().max().unwrap()))
}

fn as_const(ranges: &FxHashMap<u32, (i128, i128)>, v: u32) -> Option<i128> {
    match ranges.get(&v) {
        Some((l, h)) if l == h => Some(*l),
        _ => None,
    }
}

fn is_one(def: &[Option<&SsaOp>], ranges: &FxHashMap<u32, (i128, i128)>, v: u32) -> bool {
    if matches!(def[v as usize], Some(SsaOp::ConstInt(1))) {
        return true;
    }
    as_const(ranges, v) == Some(1)
}

struct Check {
    bound: i128,
    below: bool,
    le: bool,
}

fn header_check(
    ssa: &SsaProto,
    def: &[Option<&SsaOp>],
    ranges: &FxHashMap<u32, (i128, i128)>,
    header: usize,
    param: u32,
) -> Option<Check> {
    let blk = &ssa.blocks[header];
    let SsaTerm::Branch { cond, .. } = &blk.term else {
        return None;
    };
    let Some(SsaOp::Binary { op, lhs, rhs }) = def[*cond as usize] else {
        return None;
    };
    if !blk.params.contains(&param) {
        return None;
    }
    let le = matches!(op, SsaBinOp::IntLe | SsaBinOp::IntGe);
    match op {
        SsaBinOp::IntLt | SsaBinOp::IntLe => {
            if *lhs == param {
                Some(Check {
                    bound: as_const(ranges, *rhs)?,
                    below: true,
                    le,
                })
            } else if *rhs == param {
                Some(Check {
                    bound: as_const(ranges, *lhs)?,
                    below: false,
                    le,
                })
            } else {
                None
            }
        }
        SsaBinOp::IntGt | SsaBinOp::IntGe => {
            if *lhs == param {
                Some(Check {
                    bound: as_const(ranges, *rhs)?,
                    below: false,
                    le,
                })
            } else if *rhs == param {
                Some(Check {
                    bound: as_const(ranges, *lhs)?,
                    below: true,
                    le,
                })
            } else {
                None
            }
        }
        SsaBinOp::IntAdd
        | SsaBinOp::IntSub
        | SsaBinOp::IntMul
        | SsaBinOp::IntDiv
        | SsaBinOp::IntMod
        | SsaBinOp::IntPow
        | SsaBinOp::IntEq
        | SsaBinOp::IntNe
        | SsaBinOp::IntAnd
        | SsaBinOp::IntOr
        | SsaBinOp::IntXor
        | SsaBinOp::IntShl
        | SsaBinOp::IntShr
        | SsaBinOp::IntUshr
        | SsaBinOp::FloatAdd
        | SsaBinOp::FloatSub
        | SsaBinOp::FloatMul
        | SsaBinOp::FloatDiv
        | SsaBinOp::FloatMod
        | SsaBinOp::FloatPow
        | SsaBinOp::FloatEq
        | SsaBinOp::FloatNe
        | SsaBinOp::FloatLt
        | SsaBinOp::FloatLe
        | SsaBinOp::FloatGt
        | SsaBinOp::FloatGe
        | SsaBinOp::StrConcat
        | SsaBinOp::StrEq
        | SsaBinOp::StrNe
        | SsaBinOp::Dyn(_) => None,
    }
}

fn apply_cap(r: (i128, i128), c: &Check) -> Option<(i128, i128)> {
    let (l, h) = r;
    let (l, h) = if c.below {
        (l, h.min(if c.le { c.bound } else { c.bound - 1 }))
    } else {
        (l.max(if c.le { c.bound } else { c.bound + 1 }), h)
    };
    if l > h {
        None
    } else {
        Some((l, h))
    }
}

pub(super) fn loop_sets(
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
) -> (Vec<usize>, FxHashMap<usize, rustc_hash::FxHashSet<usize>>) {
    let def = def_map(ssa);
    let mut headers: Vec<usize> = Vec::new();
    for header in 0..ssa.blocks.len() {
        if !reached[header] {
            continue;
        }
        if super::cfg::loop_body(preds, rpo_pos, reached, header).is_empty() {
            continue;
        }
        let blk = &ssa.blocks[header];
        let SsaTerm::Branch {
            cond,
            then_blk,
            else_blk,
            ..
        } = &blk.term
        else {
            continue;
        };
        let body = super::cfg::loop_body(preds, rpo_pos, reached, header);
        if !body.contains(&(*then_blk as usize)) || body.contains(&(*else_blk as usize)) {
            continue;
        }
        if !blk.insts.iter().any(|i| i.dest == Some(*cond)) {
            continue;
        }
        if !matches!(
            def[*cond as usize],
            Some(SsaOp::Binary {
                op: SsaBinOp::IntLt | SsaBinOp::IntLe | SsaBinOp::IntGt | SsaBinOp::IntGe,
                ..
            })
        ) {
            continue;
        }
        headers.push(header);
    }
    let bodies: FxHashMap<usize, rustc_hash::FxHashSet<usize>> = headers
        .iter()
        .map(|&h| (h, super::cfg::loop_body(preds, rpo_pos, reached, h)))
        .collect();
    (headers, bodies)
}

pub(super) fn analyze(
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
) -> (FxHashSet<u32>, Vec<VersionPlan>) {
    let def = def_map(ssa);
    let dblk = def_block(ssa);
    let pred_edges: Vec<Vec<(u32, Vec<u32>, Edge)>> = (0..ssa.blocks.len())
        .map(|from| match &ssa.blocks[from].term {
            SsaTerm::Jump { target, args } => vec![(*target, args.clone(), Edge::Jump)],
            SsaTerm::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => vec![
                (*then_blk, then_args.clone(), Edge::Then),
                (*else_blk, else_args.clone(), Edge::Else),
            ],
            SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => vec![],
        })
        .collect();

    let (headers, bodies) = loop_sets(ssa, preds, rpo_pos, reached);

    let mut ranges: FxHashMap<u32, (i128, i128)> = FxHashMap::default();
    for (v, op) in def.iter().enumerate() {
        if let Some(SsaOp::ConstInt(c)) = op
            && ssa.value_ty(v as u32) == SlotKind::Int
        {
            ranges.insert(v as u32, (*c as i128, *c as i128));
        }
    }

    let mut converged = false;
    for _ in 0..64 {
        let mut changed = false;
        let mut checks: FxHashMap<(usize, u32), Check> = FxHashMap::default();
        for &h in &headers {
            for &p in &ssa.blocks[h].params {
                if ssa.value_ty(p) != SlotKind::Int {
                    continue;
                }
                if let Some(c) = header_check(ssa, &def, &ranges, h, p) {
                    checks.insert((h, p), c);
                }
            }
        }
        for (b, blk) in ssa.blocks.iter().enumerate() {
            if !reached[b] {
                continue;
            }
            for (pi, &p) in blk.params.iter().enumerate() {
                if ssa.value_ty(p) != SlotKind::Int {
                    continue;
                }
                let latched = bodies.contains_key(&b);
                let mut elo: Option<i128> = None;
                let mut ehi: Option<i128> = None;
                let mut ok = true;
                for pred in preds[b].iter().filter(|p| reached[**p]) {
                    let pred_in_body = bodies.get(&b).is_some_and(|bd| bd.contains(pred));
                    if latched && pred_in_body {
                        continue;
                    }
                    for (t, args, _) in pred_edges[*pred].iter() {
                        if *t as usize != b {
                            continue;
                        }
                        let Some(&a) = args.get(pi) else {
                            ok = false;
                            continue;
                        };
                        match ranges.get(&a) {
                            Some(&(l, h)) => {
                                elo = Some(elo.map_or(l, |x: i128| x.min(l)));
                                ehi = Some(ehi.map_or(h, |x: i128| x.max(h)));
                            }
                            None => {
                                ok = false;
                            }
                        }
                    }
                }
                if !ok || elo.is_none() {
                    continue;
                }
                let mut l = elo.unwrap();
                let mut h = ehi.unwrap();
                let entry = (l, h);
                if latched {
                    for pred in preds[b].iter().filter(|p| reached[**p]) {
                        let pred_in_body = bodies.get(&b).is_some_and(|bd| bd.contains(pred));
                        if !pred_in_body {
                            continue;
                        }
                        for (t, args, _) in pred_edges[*pred].iter() {
                            if *t as usize != b {
                                continue;
                            }
                            let Some(&a) = args.get(pi) else {
                                ok = false;
                                continue;
                            };
                            if a == p {
                                continue;
                            }
                            if let Some((cl, ch)) =
                                step_contrib(ssa, &def, &ranges, &checks, b, p, a, entry)
                            {
                                l = l.min(cl);
                                h = h.max(ch);
                                continue;
                            }
                            match ranges.get(&a) {
                                Some(&(al, ah)) => {
                                    l = l.min(al);
                                    h = h.max(ah);
                                }
                                None => {
                                    ok = false;
                                }
                            }
                        }
                    }
                    if !ok {
                        continue;
                    }
                }
                if l > h {
                    continue;
                }
                if ranges.get(&p) != Some(&(l, h)) {
                    ranges.insert(p, (l, h));
                    changed = true;
                }
            }
            for inst in &blk.insts {
                let (Some(d), SsaOp::Binary { op, lhs, rhs }) = (inst.dest, &inst.op) else {
                    continue;
                };
                if ssa.value_ty(d) != SlotKind::Int {
                    continue;
                }
                let (Some(&a), Some(&c)) = (ranges.get(lhs), ranges.get(rhs)) else {
                    continue;
                };
                let r = match op {
                    SsaBinOp::IntAdd => add_range(a, c),
                    SsaBinOp::IntSub => sub_range(a, c),
                    SsaBinOp::IntMul => mul_range(a, c),
                    SsaBinOp::IntDiv
                    | SsaBinOp::IntMod
                    | SsaBinOp::IntPow
                    | SsaBinOp::IntEq
                    | SsaBinOp::IntNe
                    | SsaBinOp::IntLt
                    | SsaBinOp::IntLe
                    | SsaBinOp::IntGt
                    | SsaBinOp::IntGe
                    | SsaBinOp::IntAnd
                    | SsaBinOp::IntOr
                    | SsaBinOp::IntXor
                    | SsaBinOp::IntShl
                    | SsaBinOp::IntShr
                    | SsaBinOp::IntUshr
                    | SsaBinOp::FloatAdd
                    | SsaBinOp::FloatSub
                    | SsaBinOp::FloatMul
                    | SsaBinOp::FloatDiv
                    | SsaBinOp::FloatMod
                    | SsaBinOp::FloatPow
                    | SsaBinOp::FloatEq
                    | SsaBinOp::FloatNe
                    | SsaBinOp::FloatLt
                    | SsaBinOp::FloatLe
                    | SsaBinOp::FloatGt
                    | SsaBinOp::FloatGe
                    | SsaBinOp::StrConcat
                    | SsaBinOp::StrEq
                    | SsaBinOp::StrNe
                    | SsaBinOp::Dyn(_) => None,
                };
                if let Some(r) = r
                    && ranges.get(&d) != Some(&r)
                {
                    ranges.insert(d, r);
                    changed = true;
                }
            }
        }
        if !changed {
            converged = true;
            break;
        }
    }

    let no_overflow: FxHashSet<u32> = if converged {
        ranges
            .iter()
            .filter(|&(_, &(l, h))| l >= I64_LO && h <= I64_HI)
            .filter(|(v, _)| {
                matches!(def[**v as usize], Some(SsaOp::Binary { op, .. }) if matches!(op, SsaBinOp::IntAdd | SsaBinOp::IntSub | SsaBinOp::IntMul))
            })
            .map(|(v, _)| *v)
            .collect()
    } else {
        FxHashSet::default()
    };

    let plans = if converged {
        version_plans(ssa, preds, reached, &def, &dblk, &bodies, &headers, &ranges)
    } else {
        Vec::new()
    };

    if super::super::trace() {
        eprintln!(
            "clif: ranges headers={} known={} no_overflow={} versions={}",
            headers.len(),
            ranges.len(),
            no_overflow.len(),
            plans.len()
        );
    }
    (no_overflow, plans)
}

fn step_contrib(
    _ssa: &SsaProto,
    def: &[Option<&SsaOp>],
    ranges: &FxHashMap<u32, (i128, i128)>,
    checks: &FxHashMap<(usize, u32), Check>,
    header: usize,
    param: u32,
    arg: u32,
    entry: (i128, i128),
) -> Option<(i128, i128)> {
    let c = checks.get(&(header, param))?;
    let (elo, ehi) = entry;
    let Some(SsaOp::Binary {
        op: step,
        lhs: sl,
        rhs: sr,
    }) = def[arg as usize]
    else {
        return None;
    };
    let is_inc = matches!(step, SsaBinOp::IntAdd)
        && ((*sl == param && is_one(def, ranges, *sr))
            || (*sr == param && is_one(def, ranges, *sl)));
    let is_dec = matches!(step, SsaBinOp::IntSub) && *sl == param && is_one(def, ranges, *sr);
    if c.below && is_inc {
        let top = if c.le { c.bound + 1 } else { c.bound };
        Some((elo + 1, top))
    } else if !c.below && is_dec {
        let floor = if c.le { c.bound - 1 } else { c.bound };
        Some((floor, ehi - 1))
    } else {
        None
    }
}

pub(super) fn version_op_ok(ssa: &SsaProto, op: &SsaOp) -> bool {
    if super::views::keeps_views(ssa, op) {
        return true;
    }
    matches!(
        op,
        SsaOp::SetFixedField { .. } | SsaOp::LoadGlobalIdx(_) | SsaOp::StoreGlobalIdx { .. }
    )
}

fn version_plans(
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    reached: &[bool],
    def: &[Option<&SsaOp>],
    dblk: &[Option<usize>],
    bodies: &FxHashMap<usize, rustc_hash::FxHashSet<usize>>,
    headers: &[usize],
    ranges: &FxHashMap<u32, (i128, i128)>,
) -> Vec<VersionPlan> {
    let _ = def;
    let mut narrowed: FxHashMap<u32, (i128, i128)> = ranges.clone();
    let mut checks: FxHashMap<(usize, u32), Check> = FxHashMap::default();
    let mut nconverged = false;
    for _ in 0..64 {
        let mut changed = false;
        checks.clear();
        for &h in headers {
            for &p in &ssa.blocks[h].params {
                if ssa.value_ty(p) != SlotKind::Int {
                    continue;
                }
                if let Some(c) = header_check(ssa, def, &narrowed, h, p) {
                    checks.insert((h, p), c);
                }
            }
        }
        for &h in headers {
            let body = &bodies[&h];
            for &b in body {
                if !reached[b] || b == h {
                    continue;
                }
                let blk = &ssa.blocks[b];
                for inst in &blk.insts {
                    let (Some(d), SsaOp::Binary { op, lhs, rhs }) = (inst.dest, &inst.op) else {
                        continue;
                    };
                    if ssa.value_ty(d) != SlotKind::Int {
                        continue;
                    }
                    if !matches!(op, SsaBinOp::IntAdd | SsaBinOp::IntSub | SsaBinOp::IntMul) {
                        continue;
                    }
                    let read = |v: u32| -> Option<(i128, i128)> {
                        let mut r = *narrowed.get(&v)?;
                        for ((hh, kk), c) in checks.iter() {
                            if *kk == v && bodies[hh].contains(&b) && b != *hh {
                                r = apply_cap(r, c)?;
                            }
                        }
                        Some(r)
                    };
                    let (Some(a), Some(c)) = (read(*lhs), read(*rhs)) else {
                        continue;
                    };
                    let r = match op {
                        SsaBinOp::IntAdd => add_range(a, c),
                        SsaBinOp::IntSub => sub_range(a, c),
                        SsaBinOp::IntMul
                        | SsaBinOp::IntDiv
                        | SsaBinOp::IntMod
                        | SsaBinOp::IntPow
                        | SsaBinOp::IntEq
                        | SsaBinOp::IntNe
                        | SsaBinOp::IntLt
                        | SsaBinOp::IntLe
                        | SsaBinOp::IntGt
                        | SsaBinOp::IntGe
                        | SsaBinOp::IntAnd
                        | SsaBinOp::IntOr
                        | SsaBinOp::IntXor
                        | SsaBinOp::IntShl
                        | SsaBinOp::IntShr
                        | SsaBinOp::IntUshr
                        | SsaBinOp::FloatAdd
                        | SsaBinOp::FloatSub
                        | SsaBinOp::FloatMul
                        | SsaBinOp::FloatDiv
                        | SsaBinOp::FloatMod
                        | SsaBinOp::FloatPow
                        | SsaBinOp::FloatEq
                        | SsaBinOp::FloatNe
                        | SsaBinOp::FloatLt
                        | SsaBinOp::FloatLe
                        | SsaBinOp::FloatGt
                        | SsaBinOp::FloatGe
                        | SsaBinOp::StrConcat
                        | SsaBinOp::StrEq
                        | SsaBinOp::StrNe
                        | SsaBinOp::Dyn(_) => mul_range(a, c),
                    };
                    if let Some(r) = r
                        && narrowed.get(&d) != Some(&r)
                    {
                        if r.0 > r.1 {
                            return Vec::new();
                        }
                        narrowed.insert(d, r);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            nconverged = true;
            break;
        }
    }
    if !nconverged {
        return Vec::new();
    }

    let use_range = |v: u32, b: usize| -> Option<(i128, i128)> {
        let mut r = *narrowed.get(&v)?;
        for ((hh, kk), c) in checks.iter() {
            if *kk == v && bodies[hh].contains(&b) && b != *hh {
                r = apply_cap(r, c)?;
            }
        }
        Some(r)
    };

    let mut plans = Vec::new();
    for &h in headers {
        if headers
            .iter()
            .any(|&h2| h2 != h && bodies[&h].contains(&h2))
        {
            continue;
        }
        let body = &bodies[&h];
        let mut ok = true;
        let mut elide = FxHashSet::default();
        let mut arrays: FxHashMap<u32, i128> = FxHashMap::default();
        for &b in body {
            if !reached[b] {
                continue;
            }
            let blk = &ssa.blocks[b];
            match &blk.term {
                SsaTerm::Jump { .. } | SsaTerm::Branch { .. } => {}
                SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => {
                    ok = false;
                    break;
                }
            }
            for inst in &blk.insts {
                if !version_op_ok(ssa, &inst.op) {
                    ok = false;
                    break;
                }
                if matches!(inst.op, SsaOp::StoreGlobalIdx { .. }) {
                    ok = false;
                    break;
                }
                match &inst.op {
                    SsaOp::ArrayGetIndex { object, index }
                    | SsaOp::BytesGetIndex { object, index }
                    | SsaOp::ArraySetIndex { object, index, .. } => {
                        if ssa.value_ty(*index) != SlotKind::Int {
                            ok = false;
                            break;
                        }
                        if b == h {
                            ok = false;
                            break;
                        }
                        let Some((lo, hi)) = use_range(*index, b) else {
                            ok = false;
                            break;
                        };
                        if lo < 0 || hi + 1 > I64_HI {
                            ok = false;
                            break;
                        }
                        if dblk[*object as usize].is_none_or(|db| body.contains(&db)) {
                            ok = false;
                            break;
                        }
                        elide.insert(*index);
                        arrays
                            .entry(*object)
                            .and_modify(|m| *m = (*m).max(hi + 1))
                            .or_insert(hi + 1);
                    }
                    SsaOp::ConstInt(_)
                    | SsaOp::ConstFloat(_)
                    | SsaOp::ConstBool(_)
                    | SsaOp::ConstNull
                    | SsaOp::ConstStr(_)
                    | SsaOp::Binary { .. }
                    | SsaOp::Unary { .. }
                    | SsaOp::SelfCall { .. }
                    | SsaOp::Call { .. }
                    | SsaOp::AllocInstance { .. }
                    | SsaOp::LoadGlobalIdx(_)
                    | SsaOp::ConstChar(_)
                    | SsaOp::ConstBigInt(_)
                    | SsaOp::ConstDecimal(_)
                    | SsaOp::MakeEnumVariant { .. }
                    | SsaOp::IntrinsicCall { .. }
                    | SsaOp::LoadNativeGlobalIdx(_)
                    | SsaOp::Try { .. }
                    | SsaOp::PopTry
                    | SsaOp::CatchParam { .. }
                    | SsaOp::StoreGlobalIdx { .. }
                    | SsaOp::MakeClosure { .. }
                    | SsaOp::LoadCaptured { .. }
                    | SsaOp::StoreCaptured { .. }
                    | SsaOp::LoadUpvalue(_)
                    | SsaOp::StoreUpvalue { .. }
                    | SsaOp::CloseUpvalues { .. }
                    | SsaOp::Cast { .. }
                    | SsaOp::Convert { .. }
                    | SsaOp::IsNull { .. }
                    | SsaOp::Typeof { .. }
                    | SsaOp::ToString { .. }
                    | SsaOp::IsArray { .. }
                    | SsaOp::GetEnumTag { .. }
                    | SsaOp::ObjectKeys { .. }
                    | SsaOp::BuildStr { .. }
                    | SsaOp::BuildArray { .. }
                    | SsaOp::BuildMap { .. }
                    | SsaOp::BuildObject { .. }
                    | SsaOp::GetProperty { .. }
                    | SsaOp::SetProperty { .. }
                    | SsaOp::GetIndex { .. }
                    | SsaOp::SetIndex { .. }
                    | SsaOp::ArrayLength { .. }
                    | SsaOp::StrLength { .. }
                    | SsaOp::BytesLength { .. }
                    | SsaOp::MethodCall { .. }
                    | SsaOp::CallNativeOp { .. }
                    | SsaOp::ArrayPush { .. }
                    | SsaOp::This
                    | SsaOp::GetFixedField { .. }
                    | SsaOp::SetFixedField { .. }
                    | SsaOp::MakeClass { .. }
                    | SsaOp::DeclareLayout { .. }
                    | SsaOp::DefineMethod { .. }
                    | SsaOp::GetSuper { .. }
                    | SsaOp::LoadGlobal(_)
                    | SsaOp::StoreGlobal { .. }
                    | SsaOp::BuildTuple { .. }
                    | SsaOp::BuildArraySpread { .. }
                    | SsaOp::BuildObjectSpread { .. }
                    | SsaOp::ObjectMerge { .. }
                    | SsaOp::ObjectRest { .. }
                    | SsaOp::GetPropertyMaybe { .. }
                    | SsaOp::AssertNotNull { .. }
                    | SsaOp::BindMethod { .. }
                    | SsaOp::ArrayExtend { .. }
                    | SsaOp::WrapSpread { .. }
                    | SsaOp::Range { .. }
                    | SsaOp::GetSymbol { .. }
                    | SsaOp::IterCall { .. }
                    | SsaOp::SuperCall { .. }
                    | SsaOp::SuperMethodCall { .. }
                    | SsaOp::ExtensionCall { .. }
                    | SsaOp::CallSpread { .. }
                    | SsaOp::LoadModule { .. }
                    | SsaOp::ModuleSlot { .. }
                    | SsaOp::StoreModuleSlot { .. }
                    | SsaOp::Await { .. }
                    | SsaOp::Spawn { .. }
                    | SsaOp::Yield { .. }
                    | SsaOp::Dispose { .. } => {}
                }
            }
            if !ok {
                break;
            }
        }
        if !ok || arrays.is_empty() {
            continue;
        }
        let outside: Vec<usize> = preds[h]
            .iter()
            .copied()
            .filter(|p| reached[*p] && !body.contains(p))
            .collect();
        if outside.len() != 1 {
            continue;
        }
        let pre = outside[0];
        if !matches!(
            ssa.blocks[pre].term,
            SsaTerm::Jump { target, .. } if target as usize == h
        ) {
            continue;
        }
        plans.push(VersionPlan {
            header: h,
            preheader: pre,
            guards: arrays.into_iter().collect(),
            elide,
        });
    }
    plans
}
