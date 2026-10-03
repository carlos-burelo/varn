#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SuspendLive {
    pub resume_ip: u32,
    pub regs: Vec<u16>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ExceptionRange {
    pub try_start_ip: u32,
    pub try_end_ip: u32,
    pub catch_ip: u32,
    pub err_reg: u8,
}
