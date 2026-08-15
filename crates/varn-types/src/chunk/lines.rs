//! Bytecode offset -> source line mapping, run-length encoded.

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
pub struct LineEntry {
    pub count: u32,
    pub line: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash, Default)]
pub struct LineMapping {
    pub entries: Vec<LineEntry>,

    #[serde(skip)]
    starts: Vec<u32>,
}

impl LineMapping {
    pub fn add(&mut self, line: u32) {
        if let Some(last) = self.entries.last_mut() {
            if last.line == line {
                last.count += 1;

                return;
            }
        }

        let next_start: u32 = self.starts.last().copied().unwrap_or(0)
            + self.entries.last().map(|e| e.count).unwrap_or(0);
        self.starts.push(next_start);
        self.entries.push(LineEntry { count: 1, line });
    }

    pub fn get_line(&self, instruction_idx: usize) -> u32 {
        if self.starts.len() != self.entries.len() {
            let mut base = 0usize;
            for entry in &self.entries {
                let next = base + entry.count as usize;
                if instruction_idx < next {
                    return entry.line;
                }
                base = next;
            }
            return 0;
        }
        let idx = instruction_idx as u32;

        let pos = self.starts.partition_point(|&s| s <= idx);
        if pos == 0 {
            return 0;
        }
        self.entries[pos - 1].line
    }

    pub fn truncate(&mut self, instruction_idx: usize) {
        let mut current = 0;
        let mut to_remove_from = None;
        for (i, entry) in self.entries.iter_mut().enumerate() {
            let next = current + entry.count as usize;
            if instruction_idx < next {
                let keep = instruction_idx - current;
                if keep == 0 {
                    to_remove_from = Some(i);
                } else {
                    entry.count = keep as u32;
                    to_remove_from = Some(i + 1);
                }
                break;
            }
            current = next;
        }
        if let Some(idx) = to_remove_from {
            self.entries.truncate(idx);
            self.starts.truncate(idx);
        }
    }
}
