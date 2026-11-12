pub struct OpMeta {
    pub name: &'static str,
    pub op_id: u64,
    pub is_async: bool,
    pub capability: Option<&'static str>,
}

pub struct ModuleMeta {
    pub module: &'static str,
    pub ops: &'static [OpMeta],
}
