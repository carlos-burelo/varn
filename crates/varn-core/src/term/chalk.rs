use std::fmt::{self, Display, Formatter};

#[derive(Clone, Debug)]
pub struct Chalk {
    text: String,
    style: console::Style,
}

impl Chalk {
    fn new(text: impl Display, style: console::Style) -> Self {
        Self {
            text: text.to_string(),
            style,
        }
    }

    fn styled(mut self, f: impl FnOnce(console::Style) -> console::Style) -> Self {
        self.style = f(self.style);
        self
    }

    pub fn red(self) -> Self {
        self.styled(|s| s.red())
    }
    pub fn green(self) -> Self {
        self.styled(|s| s.green())
    }
    pub fn yellow(self) -> Self {
        self.styled(|s| s.yellow())
    }
    pub fn blue(self) -> Self {
        self.styled(|s| s.blue())
    }
    pub fn magenta(self) -> Self {
        self.styled(|s| s.magenta())
    }
    pub fn cyan(self) -> Self {
        self.styled(|s| s.cyan())
    }
    pub fn white(self) -> Self {
        self.styled(|s| s.white())
    }

    pub fn bold(self) -> Self {
        self.styled(|s| s.bold())
    }
    pub fn dim(self) -> Self {
        self.styled(|s| s.dim())
    }
    pub fn underline(self) -> Self {
        self.styled(|s| s.underlined())
    }
}

impl Display for Chalk {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let forced = self.style.clone().force_styling(true).for_stderr();
        write!(f, "{}", forced.apply_to(&self.text))
    }
}

pub fn chalk(text: impl Display) -> Chalk {
    Chalk::new(text, console::Style::new())
}

pub fn chalk_fmt(args: fmt::Arguments<'_>) -> Chalk {
    Chalk::new(args, console::Style::new())
}
