use std::collections::{HashMap, HashSet};

use super::ir::IrModule;

#[derive(Debug, Clone)]
pub struct LiveRange {
    pub vreg: u16,
    pub start: usize,
    pub end: usize,
    pub interference: Vec<u16>,
}

pub struct LivenessAnalyzer {
    def_sites: HashMap<u16, usize>,
    use_sites: HashMap<u16, Vec<usize>>,
    live_ranges: Vec<LiveRange>,
}

impl LivenessAnalyzer {
    pub fn new() -> Self {
        Self {
            def_sites: HashMap::new(),
            use_sites: HashMap::new(),
            live_ranges: Vec::new(),
        }
    }

    pub fn analyze_module(&mut self, module: &IrModule) -> Vec<LiveRange> {
        self.def_sites.clear();
        self.use_sites.clear();

        for (idx, instr) in module.instrs.iter().enumerate() {
            if !instr.dest.is_none() {
                self.def_sites.insert(instr.dest.0, idx);
            }
            if !instr.src1.is_none() {
                self.use_sites
                    .entry(instr.src1.0)
                    .or_insert_with(Vec::new)
                    .push(idx);
            }
            if !instr.src2.is_none() {
                self.use_sites
                    .entry(instr.src2.0)
                    .or_insert_with(Vec::new)
                    .push(idx);
            }
        }

        let vregs = module.used_vregs();
        self.analyze(vregs)
    }

    pub fn analyze(&mut self, vregs_used: Vec<u16>) -> Vec<LiveRange> {
        self.analyze_with_back_edges(vregs_used, &[])
    }

    /// Like `analyze`, but extends live ranges across loop back-edges.
    /// `back_edges` is a list of `(loop_header, loop_end)` pairs where
    /// `loop_header` is the first instruction of the loop body and
    /// `loop_end` is the position of the `Loop` instruction itself.
    pub fn analyze_with_back_edges(
        &mut self,
        vregs_used: Vec<u16>,
        back_edges: &[(usize, usize)],
    ) -> Vec<LiveRange> {
        self.live_ranges.clear();

        for vreg in vregs_used {
            if let (Some(&start_val), Some(uses)) =
                (self.def_sites.get(&vreg), self.use_sites.get(&vreg))
            {
                let mut start = start_val;
                let mut end = uses.iter().copied().max().unwrap_or(start);

                // For each back-edge (header, loop_end):
                // If this vreg is live anywhere inside the loop body [header..=loop_end],
                // extend its range to cover the entire loop body so that the register
                // stays allocated across iterations.
                let mut changed = true;
                while changed {
                    changed = false;
                    for &(header, loop_end) in back_edges {
                        let live_in_loop = start <= loop_end && end >= header;
                        if live_in_loop {
                            if start > header {
                                // Def is inside loop — extend start to header.
                                // (Conservative: treat as live from loop entry.)
                                start = header;
                                changed = true;
                            }
                            if end < loop_end {
                                end = loop_end;
                                changed = true;
                            }
                        }
                    }
                }

                self.live_ranges.push(LiveRange {
                    vreg,
                    start,
                    end,
                    interference: Vec::new(),
                });
            }
        }

        self.compute_interference();
        self.live_ranges.clone()
    }

    pub fn record_def(&mut self, vreg: u16, instr_idx: usize) {
        self.def_sites.insert(vreg, instr_idx);
    }

    pub fn record_use(&mut self, vreg: u16, instr_idx: usize) {
        self.use_sites
            .entry(vreg)
            .or_insert_with(Vec::new)
            .push(instr_idx);
    }

    pub fn max_concurrent_live(&self) -> usize {
        if self.live_ranges.is_empty() {
            return 0;
        }
        // Sweep-line: O((L+N) log L) instead of O(L×N).
        let mut events: Vec<(usize, bool)> = Vec::with_capacity(self.live_ranges.len() * 2);
        for r in &self.live_ranges {
            events.push((r.start, false)); // false = start
            events.push((r.end + 1, true)); // true = end (exclusive)
        }
        // Sort by point; on tie, ends before starts so we don't over-count.
        events.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        let mut live = 0i64;
        let mut max_live = 0usize;
        for (_, is_end) in events {
            if is_end {
                live -= 1;
            } else {
                live += 1;
                if live as usize > max_live {
                    max_live = live as usize;
                }
            }
        }
        max_live
    }

    fn compute_interference(&mut self) {
        let ranges_len = self.live_ranges.len();
        let mut pairs: Vec<(usize, usize)> = Vec::new();

        for i in 0..ranges_len {
            for j in (i + 1)..ranges_len {
                let r1 = &self.live_ranges[i];
                let r2 = &self.live_ranges[j];

                if !(r1.end < r2.start || r2.end < r1.start) {
                    pairs.push((i, j));
                }
            }
        }

        for (i, j) in pairs {
            let vi = self.live_ranges[i].vreg;
            let vj = self.live_ranges[j].vreg;
            self.live_ranges[i].interference.push(vj);
            self.live_ranges[j].interference.push(vi);
        }
    }

    pub fn live_ranges(&self) -> &[LiveRange] {
        &self.live_ranges
    }
}

pub struct InterferenceGraph {
    pub nodes: Vec<u16>,
    pub edges: HashMap<u16, HashSet<u16>>,
}

impl InterferenceGraph {
    pub fn from_live_ranges(ranges: &[LiveRange]) -> Self {
        let mut nodes = Vec::new();
        let mut edges: HashMap<u16, HashSet<u16>> = HashMap::new();

        for range in ranges {
            nodes.push(range.vreg);
            edges.insert(range.vreg, range.interference.iter().copied().collect());
        }

        Self { nodes, edges }
    }

    pub fn chromatic_number_upper_bound(&self) -> usize {
        if self.nodes.is_empty() {
            return 0;
        }
        // Largest-degree-first ordering gives a tighter upper bound than insertion order.
        let mut ordered = self.nodes.clone();
        ordered.sort_unstable_by(|a, b| {
            let da = self.edges.get(a).map(|s| s.len()).unwrap_or(0);
            let db = self.edges.get(b).map(|s| s.len()).unwrap_or(0);
            db.cmp(&da)
        });

        let mut coloring: HashMap<u16, usize> = HashMap::new();
        let mut max_color = 0usize;

        for &node in &ordered {
            let neighbor_colors: HashSet<usize> = self
                .edges
                .get(&node)
                .map(|neighbors| {
                    neighbors
                        .iter()
                        .filter_map(|n| coloring.get(n).copied())
                        .collect()
                })
                .unwrap_or_default();

            let color = (0..).find(|c| !neighbor_colors.contains(c)).unwrap_or(0);
            coloring.insert(node, color);
            if color > max_color {
                max_color = color;
            }
        }

        max_color + 1
    }

    pub fn find_low_degree_node(&self, max_colors: u16) -> Option<u16> {
        for &node in &self.nodes {
            let degree = self.edges.get(&node).map(|s| s.len()).unwrap_or(0);
            if degree < max_colors as usize {
                return Some(node);
            }
        }
        None
    }
}
