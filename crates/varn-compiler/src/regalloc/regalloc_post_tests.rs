//! Contract tests for the coalescing colourer.
//!
//! The shapes below are lifted from a real miscompile: an object literal with
//! two fields passed to a function whose result feeds an intrinsic. The
//! colourer had no legal colour left for the object's destination and handed
//! out `base` anyway, aliasing it onto a register that was still live.

use std::collections::HashMap;
use crate::regalloc::liveness::{DefSites, LiveRange};
use crate::regalloc::regalloc_post::*;

fn range(vreg: u16, start: usize, end: usize, interference: &[u16]) -> LiveRange {
    LiveRange {
        vreg,
        start,
        end,
        interference: interference.to_vec(),
    }
}

/// `{ id: 7, extra: "z" }` built into r1..r2, consumed by a call whose argument
/// window is r7..r8, with the object's destination r3 live across that call.
///
/// r1 takes colour 1 (and r2 colour 2, being its block child), r7 takes 3 (r8
/// takes 4). r3 interferes with all of them, and the callee-frame ceiling caps
/// it at 2 because it is live across the call at r7. Nothing is left.
fn infeasible_case() -> (Vec<LiveRange>, ScanResult, Vec<(u8, u8)>) {
    let ranges = vec![
        range(1, 0, 7, &[2, 3, 7]),
        range(2, 3, 12, &[1, 3, 7, 8]),
        range(3, 4, 11, &[1, 2, 7, 8]),
        range(7, 6, 12, &[1, 2, 3, 8]),
        range(8, 6, 12, &[1, 2, 3, 7]),
    ];
    let scan = ScanResult {
        defs: [
            (1, DefSites::at(0)),
            (2, DefSites::at(3)),
            (3, DefSites::at(4)),
            (7, DefSites::at(6)),
            (8, DefSites::at(6)),
        ]
        .into_iter()
        .collect(),
        uses: [
            (1, vec![7]),
            (2, vec![12]),
            (3, vec![6, 11]),
            (7, vec![12]),
            (8, vec![12]),
        ]
        .into_iter()
        .collect(),
        call_sites: vec![(9, 7, 2)],
    };
    let blocks = vec![(1, 2), (7, 2)];
    (ranges, scan, blocks)
}

#[test]
fn an_infeasible_register_abandons_the_function_instead_of_aliasing() {
    let (ranges, scan, blocks) = infeasible_case();
    assert_eq!(
        color_with_base(&ranges, 1, &[], &scan, &blocks),
        None,
        "no colour satisfies interference and the callee-frame ceiling at once; \
         the colourer must say so rather than pick one that violates either"
    );
}

/// Same shape without the call, so the ceiling disappears and r3 has room.
/// The point is not the specific colours — it is that whatever comes out
/// respects the interference the analyser computed.
#[test]
fn a_feasible_colouring_never_aliases_interfering_registers() {
    let (ranges, mut scan, blocks) = infeasible_case();
    scan.call_sites.clear();

    let coloring = color_with_base(&ranges, 1, &[], &scan, &blocks)
        .expect("removing the ceiling leaves colours available");

    let mapping: HashMap<u8, u8> = coloring.into_iter().filter(|&(o, n)| o != n).collect();
    assert!(
        verify_interference(&ranges, &mapping),
        "colourer produced {mapping:?}, which aliases two overlapping ranges"
    );
}

/// The verifier is the backstop for the bug above, so it has to actually
/// reject the mapping the old colourer produced: r3 folded onto r1's colour.
#[test]
fn the_verifier_rejects_the_mapping_the_old_colourer_produced() {
    let (ranges, _, _) = infeasible_case();
    let aliased: HashMap<u8, u8> = [(3, 1)].into_iter().collect();
    assert!(!verify_interference(&ranges, &aliased));
}
