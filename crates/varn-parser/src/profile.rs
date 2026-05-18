use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct ParseProfile {
    pub program_loop: Duration,
    pub stmt_or_decl: Duration,
    pub block: Duration,
    pub recover: Duration,
}
