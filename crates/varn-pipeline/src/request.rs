#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Goal {
    Tokens,
    Ast,
    Checked,
    Executable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Strict,
    Tooling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CachePolicy {
    Use,
    Bypass,
}

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub goal: Goal,
    pub cache: CachePolicy,
    pub mode: Mode,
    pub measure_optimizer: bool,
}

impl Request {
    pub fn executable(cache: CachePolicy) -> Request {
        Request {
            goal: Goal::Executable,
            cache,
            mode: Mode::Strict,
            measure_optimizer: false,
        }
    }

    pub fn measured(self) -> Request {
        Request {
            measure_optimizer: true,
            ..self
        }
    }

    pub fn tooling() -> Request {
        Request {
            goal: Goal::Checked,
            cache: CachePolicy::Bypass,
            mode: Mode::Tooling,
            measure_optimizer: false,
        }
    }
}
