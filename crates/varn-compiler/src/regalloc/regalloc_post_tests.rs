use crate::regalloc::liveness::{DefSites, LiveRange};
use crate::regalloc::regalloc_post::*;
use rustc_hash::FxHashMap as HashMap;
use varn_types::register_meta::SlotKind;

fn range(vreg: u16, start: usize, end: usize, interference: &[u16]) -> LiveRange {
    LiveRange {
        vreg,
        start,
        end,
        interference: interference.to_vec(),
    }
}

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
        color_with_base(&ranges, 1, &[], &scan, &blocks, &[]),
        None,
        "no colour satisfies interference and the callee-frame ceiling at once; \
         the colourer must say so rather than pick one that violates either"
    );
}

#[test]
fn a_feasible_colouring_never_aliases_interfering_registers() {
    let (ranges, mut scan, blocks) = infeasible_case();
    scan.call_sites.clear();

    let coloring = color_with_base(&ranges, 1, &[], &scan, &blocks, &[])
        .expect("removing the ceiling leaves colours available");

    let mapping: HashMap<u8, u8> = coloring.into_iter().filter(|&(o, n)| o != n).collect();
    assert!(
        verify_interference(&ranges, &mapping),
        "colourer produced {mapping:?}, which aliases two overlapping ranges"
    );
}

#[test]
fn the_verifier_rejects_the_mapping_the_old_colourer_produced() {
    let (ranges, _, _) = infeasible_case();
    let aliased: HashMap<u8, u8> = [(3, 1)].into_iter().collect();
    assert!(!verify_interference(&ranges, &aliased));
}

#[test]
fn copies_across_kinds_never_share_a_colour() {
    let ranges = vec![range(1, 0, 2, &[]), range(2, 3, 5, &[])];
    let scan = ScanResult {
        defs: [(1, DefSites::at(0)), (2, DefSites::at(3))]
            .into_iter()
            .collect(),
        uses: [(1, vec![2]), (2, vec![5])].into_iter().collect(),
        call_sites: vec![],
    };
    let kinds = vec![SlotKind::Dynamic, SlotKind::Int, SlotKind::Float];
    let coloring = color_with_base(&ranges, 1, &[(2, 1)], &scan, &[], &kinds)
        .expect("separate colours are always available here");
    assert_ne!(
        coloring[&1], coloring[&2],
        "int r1 and float r2 must keep distinct colours, got {coloring:?}"
    );
}

#[test]
fn same_kind_copies_still_coalesce() {
    let ranges = vec![range(1, 0, 2, &[]), range(2, 3, 5, &[])];
    let scan = ScanResult {
        defs: [(1, DefSites::at(0)), (2, DefSites::at(3))]
            .into_iter()
            .collect(),
        uses: [(1, vec![2]), (2, vec![5])].into_iter().collect(),
        call_sites: vec![],
    };
    let kinds = vec![SlotKind::Dynamic, SlotKind::Int, SlotKind::Int];
    let coloring = color_with_base(&ranges, 1, &[(2, 1)], &scan, &[], &kinds)
        .expect("a shared colour satisfies every constraint here");
    assert_eq!(
        coloring[&1], coloring[&2],
        "two int ranges joined by a Move should share one colour, got {coloring:?}"
    );
}
