use varn_types::capabilities::CapabilitySet;

pub struct Policy {
    pub capabilities: CapabilitySet,
    pub trace: bool,
    pub gc: bool,
    pub silent: bool,
}

impl Policy {
    pub fn new(capabilities: CapabilitySet) -> Policy {
        Policy {
            capabilities,
            trace: false,
            gc: false,
            silent: false,
        }
    }
}
