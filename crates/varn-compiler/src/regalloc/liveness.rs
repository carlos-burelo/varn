use std::collections::HashMap;

/// Puntos extremos de escritura de un registro dentro de la función.
///
/// Una escritura ocupa el registro en ese instante igual que una lectura: si el
/// rango terminase en el último USO, un registro reescrito más tarde parecería
/// libre entre medias y el coloreado podría entregar su slot a un valor que
/// sigue vivo ahí. Guardar `last` mantiene el rango cubriendo cada escritura.
#[derive(Debug, Clone, Copy)]
pub struct DefSites {
    pub first: usize,
    pub last: usize,
}

impl DefSites {
    pub fn at(idx: usize) -> Self {
        Self {
            first: idx,
            last: idx,
        }
    }

    pub fn extend(&mut self, idx: usize) {
        self.first = self.first.min(idx);
        self.last = self.last.max(idx);
    }
}

#[derive(Debug, Clone)]
pub struct LiveRange {
    pub vreg: u16,
    pub start: usize,
    pub end: usize,
    pub interference: Vec<u16>,
}

pub struct LivenessAnalyzer {
    def_sites: HashMap<u16, DefSites>,
    use_sites: HashMap<u16, Vec<usize>>,
    live_ranges: Vec<LiveRange>,
}

impl Default for LivenessAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl LivenessAnalyzer {
    pub fn new() -> Self {
        Self {
            def_sites: HashMap::new(),
            use_sites: HashMap::new(),
            live_ranges: Vec::new(),
        }
    }

    pub fn analyze_with_back_edges(
        &mut self,
        vregs_used: Vec<u16>,
        back_edges: &[(usize, usize)],
    ) -> Vec<LiveRange> {
        self.live_ranges.clear();

        for vreg in vregs_used {
            if let Some(&defs) = self.def_sites.get(&vreg) {
                let mut start = defs.first;
                let mut end = self
                    .use_sites
                    .get(&vreg)
                    .map(|uses| uses.iter().copied().max().unwrap_or(defs.last))
                    .unwrap_or(defs.last)
                    .max(defs.last);

                let mut changed = true;
                while changed {
                    changed = false;
                    for &(header, loop_end) in back_edges {
                        let live_in_loop = start <= loop_end && end >= header;
                        if live_in_loop {
                            if start > header {
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
        self.def_sites
            .entry(vreg)
            .and_modify(|existing| existing.extend(instr_idx))
            .or_insert_with(|| DefSites::at(instr_idx));
    }

    pub fn record_use(&mut self, vreg: u16, instr_idx: usize) {
        self.use_sites.entry(vreg).or_default().push(instr_idx);
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
}
