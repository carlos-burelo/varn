use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use varn_types::register_meta::SlotKind;

use super::scan::ScanResult;
use crate::regalloc::liveness::LiveRange;























pub(crate) fn color_with_base(
    ranges: &[LiveRange],
    base: u8,
    copies: &[(u8, u8)],
    scan: &ScanResult,
    blocks: &[(u8, u8)],
    kinds: &[SlotKind],
) -> Option<HashMap<u8, u8>> {
    let kind_of = |reg: u8| {
        kinds
            .get(reg as usize)
            .copied()
            .unwrap_or(SlotKind::Dynamic)
    };
    
    
    let mut color_kind: HashMap<u8, SlotKind> = HashMap::default();
    let compatible = |color_kind: &HashMap<u8, SlotKind>, color: u8, kind: SlotKind| {
        color_kind.get(&color).is_none_or(|&k| k == kind)
    };
    let mut coloring: HashMap<u8, u8> = HashMap::default();

    let ranges_by_vreg: HashMap<u8, &LiveRange> =
        ranges.iter().map(|r| (r.vreg as u8, r)).collect();

    let mut parent_of: HashMap<u8, (u8, u8)> = HashMap::default();
    let mut block_count: HashMap<u8, u8> = HashMap::default();
    for &(start, count) in blocks {
        block_count.insert(start, count);
        for i in 0..count {
            parent_of.insert(start + i, (start, i));
        }
    }

    let mut arg_starts = HashSet::default();
    for &(_, arg_start, _) in &scan.call_sites {
        arg_starts.insert(arg_start);
    }
    for &(start, _) in blocks {
        arg_starts.insert(start);
    }

    let mut sorted_representatives = Vec::new();
    for range in ranges {
        let reg = range.vreg as u8;
        if let Some(&(_parent, offset)) = parent_of.get(&reg) {
            if offset == 0 {
                sorted_representatives.push(range);
            }
        } else {
            sorted_representatives.push(range);
        }
    }

    sorted_representatives.sort_by(|a, b| {
        let a_reg = a.vreg as u8;
        let b_reg = b.vreg as u8;
        let a_is_arg = arg_starts.contains(&a_reg);
        let b_is_arg = arg_starts.contains(&b_reg);
        if a_is_arg != b_is_arg {
            b_is_arg.cmp(&a_is_arg)
        } else {
            a.start.cmp(&b.start)
        }
    });

    for range in sorted_representatives {
        let reg = range.vreg as u8;
        let count = block_count.get(&reg).copied().unwrap_or(1);

        let mut neighbor_colors = HashSet::default();
        for offset in 0..count {
            let child = reg + offset;
            if let Some(child_range) = ranges_by_vreg.get(&child) {
                for &n in &child_range.interference {
                    if let Some(&c) = coloring.get(&(n as u8)) {
                        if c >= offset {
                            neighbor_colors.insert(c - offset);
                        }
                    }
                }
            }
        }

        let mut max_allowed_color = 255;
        for offset in 0..count {
            let child = reg + offset;
            for &(call_idx, arg_start, _) in &scan.call_sites {
                let is_live_across = scan.defs.get(&child).is_some_and(|d| d.first < call_idx)
                    && scan
                        .uses
                        .get(&child)
                        .is_some_and(|us| us.iter().any(|&u| u > call_idx));
                if is_live_across {
                    if let Some(&c) = coloring.get(&arg_start) {
                        if c > offset {
                            max_allowed_color = max_allowed_color.min(c - 1 - offset);
                        } else {
                            max_allowed_color = 0;
                        }
                    }
                }
            }
        }

        let mut color_opt = None;
        for &(u, v) in copies {
            let mut target = None;
            if u == reg {
                target = coloring.get(&v).copied();
            } else if v == reg {
                target = coloring.get(&u).copied();
            }
            if let Some(c) = target {
                
                
                
                
                let ends_share_kind = kind_of(u) == kind_of(v);
                let slots_compatible =
                    (0..count).all(|off| compatible(&color_kind, c + off, kind_of(reg + off)));
                if !neighbor_colors.contains(&c)
                    && c >= base
                    && c <= max_allowed_color
                    && ends_share_kind
                    && slots_compatible
                {
                    color_opt = Some(c);
                    break;
                }
            }
        }

        let color = match color_opt {
            Some(c) => c,
            
            
            
            
            None => (base..=max_allowed_color).find(|c| {
                !neighbor_colors.contains(c)
                    && (0..count).all(|off| compatible(&color_kind, c + off, kind_of(reg + off)))
            })?,
        };

        for offset in 0..count {
            coloring.insert(reg + offset, color + offset);
            color_kind.insert(color + offset, kind_of(reg + offset));
        }
    }

    Some(coloring)
}
