use std::fmt::Write;

#[derive(Clone, Copy)]
pub enum Align {
    Left,
    Right,
}

pub struct Table {
    headers: Vec<String>,
    aligns: Vec<Align>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new<const N: usize>(headers: [&str; N], aligns: [Align; N]) -> Self {
        Self {
            headers: headers.iter().map(|h| (*h).to_owned()).collect(),
            aligns: aligns.to_vec(),
            rows: Vec::new(),
        }
    }

    pub fn pairs() -> Self {
        Self {
            headers: Vec::new(),
            aligns: vec![Align::Left, Align::Left],
            rows: Vec::new(),
        }
    }

    pub fn row<const N: usize>(&mut self, cells: [String; N]) -> &mut Self {
        self.rows.push(cells.to_vec());
        self
    }

    fn render(&self, out: &mut String) {
        let columns = self.aligns.len();
        let mut widths = vec![0usize; columns];
        let measured = std::iter::once(&self.headers)
            .filter(|h| !h.is_empty())
            .chain(self.rows.iter());
        for line in measured {
            for (width, cell) in widths.iter_mut().zip(line) {
                *width = (*width).max(cell.chars().count());
            }
        }
        let mut emit = |line: &[String]| {
            let mut text = String::new();
            for (index, cell) in line.iter().enumerate() {
                if index > 0 {
                    text.push_str("  ");
                }
                let pad = widths[index].saturating_sub(cell.chars().count());
                match self.aligns[index] {
                    Align::Left => {
                        text.push_str(cell);
                        text.extend(std::iter::repeat_n(' ', pad));
                    }
                    Align::Right => {
                        text.extend(std::iter::repeat_n(' ', pad));
                        text.push_str(cell);
                    }
                }
            }
            let _ = writeln!(out, "{}", text.trim_end());
        };
        if !self.headers.is_empty() {
            emit(&self.headers);
        }
        for row in &self.rows {
            emit(row);
        }
    }
}

#[derive(Default)]
pub struct Report {
    text: String,
}

impl Report {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(&mut self, name: &str) -> &mut Self {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        let _ = writeln!(self.text, "{name}");
        self
    }

    pub fn line(&mut self, text: impl AsRef<str>) -> &mut Self {
        let _ = writeln!(self.text, "{}", text.as_ref());
        self
    }

    pub fn table(&mut self, table: &Table) -> &mut Self {
        table.render(&mut self.text);
        self
    }

    pub fn print(self) {
        print!("{}", self.text);
    }
}
