pub(super) struct Term {
    pub(super) no_color: bool,
}

impl Term {
    pub(super) fn new(no_color: bool) -> Self {
        Self { no_color }
    }

    fn color(&self, code: &str, s: &str) -> String {
        if self.no_color {
            s.to_string()
        } else {
            format!("\x1b[{code}m{s}\x1b[0m")
        }
    }

    pub(super) fn cyan(&self, s: &str) -> String {
        self.color("36", s)
    }
    pub(super) fn bold_cyan(&self, s: &str) -> String {
        self.color("1;36", s)
    }
    pub(super) fn bold_green(&self, s: &str) -> String {
        self.color("1;32", s)
    }
    pub(super) fn bold_red(&self, s: &str) -> String {
        self.color("1;31", s)
    }
    pub(super) fn yellow(&self, s: &str) -> String {
        self.color("33", s)
    }
    pub(super) fn gray(&self, s: &str) -> String {
        self.color("90", s)
    }
    pub(super) fn white(&self, s: &str) -> String {
        self.color("37", s)
    }
    pub(super) fn bold_white(&self, s: &str) -> String {
        self.color("1;37", s)
    }

    pub(super) fn rt_color(&self, rt: &str, s: &str) -> String {
        match rt {
            "varn" => self.color("1;36", s),
            "bun" => self.color("1;33", s),
            "node" => self.color("1;32", s),
            "python" => self.color("1;34", s),
            "varn-base" => self.color("1;35", s),
            _ => self.white(s),
        }
    }

    pub(super) fn bar(&self, rt: &str, count: usize) -> String {
        let block = "█".repeat(count);
        self.rt_color(rt, &block)
    }
}
