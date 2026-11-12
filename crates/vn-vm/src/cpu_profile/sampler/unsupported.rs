use std::time::Duration;

use super::super::capture::Shared;

pub(in super::super) struct Sampler;

impl Sampler {
    pub(in super::super) fn start(_: &Shared, _: Duration) -> Result<Self, String> {
        Err("the cpu profiler is not available on this platform".to_owned())
    }

    pub(in super::super) fn stop(self) {}
}
