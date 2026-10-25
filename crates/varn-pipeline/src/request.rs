#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Goal {
    Tokens,
    Ast,
    Checked,
    Executable,
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
    pub measure_optimizer: bool,
}

impl Request {
    pub fn executable(cache: CachePolicy) -> Request {
        Request {
            goal: Goal::Executable,
            cache,
            measure_optimizer: false,
        }
    }

    pub fn checked() -> Request {
        Request {
            goal: Goal::Checked,
            cache: CachePolicy::Bypass,
            measure_optimizer: false,
        }
    }

    pub fn measured(self) -> Request {
        Request {
            measure_optimizer: true,
            ..self
        }
    }
}
