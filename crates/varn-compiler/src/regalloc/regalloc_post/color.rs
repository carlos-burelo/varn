use std::collections::{HashMap, HashSet};

use crate::regalloc::liveness::LiveRange;
use super::scan::ScanResult;

/// Re-colour the function's registers by liveness, coalescing `Move` copies.
///
/// Two constraints are hard, and both are correctness — not heuristics:
///
/// * **interference** — registers whose live ranges overlap never share a
///   colour;
/// * **callee frame** — a register live across a call is coloured below that
///   call's argument window, so the callee's frame cannot clobber it. This is
///   the `max_allowed_color` ceiling.
///
/// They can be jointly infeasible for a given assignment order: every colour
/// under the ceiling may already belong to a neighbour. There is no third
/// option — this pass cannot move an argument window — so infeasibility is
/// reported as `None` and the caller leaves the function's allocation alone.
pub(crate) fn color_with_base(
    ranges: &[LiveRange],
    base: u8,
    copies: &[(u8, u8)],
    scan: &ScanResult,
    blocks: &[(u8, u8)],
) -> Option<HashMap<u8, u8>> {
    let mut coloring: HashMap<u8, u8> = HashMap::new();

    let ranges_by_vreg: HashMap<u8, &LiveRange> =
        ranges.iter().map(|r| (r.vreg as u8, r)).collect();

    let mut parent_of: HashMap<u8, (u8, u8)> = HashMap::new();
    let mut block_count: HashMap<u8, u8> = HashMap::new();
    for &(start, count) in blocks {
        block_count.insert(start, count);
        for i in 0..count {
            parent_of.insert(start + i, (start, i));
        }
    }

    let mut arg_starts = HashSet::new();
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

        let mut neighbor_colors = HashSet::new();
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
                if !neighbor_colors.contains(&c) && c >= base && c <= max_allowed_color {
                    color_opt = Some(c);
                    break;
                }
            }
        }

        let color = match color_opt {
            Some(c) => c,
            // No colour satisfies both hard constraints at once. This pass
            // cannot widen the search — moving an argument window is the
            // caller's allocation, not ours — so the function keeps the
            // registers the SSA emitter gave it.
            None => (base..=max_allowed_color).find(|c| !neighbor_colors.contains(c))?,
        };

        for offset in 0..count {
            coloring.insert(reg + offset, color + offset);
        }
    }

    Some(coloring)
}
