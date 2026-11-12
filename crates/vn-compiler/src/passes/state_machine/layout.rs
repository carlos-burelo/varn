use crate::ssa::suspend::SuspendPoint;

#[derive(Debug, Clone)]
pub struct StateLayout {
    pub state_size: u16,
}

impl StateLayout {
    pub fn compute(points: &[SuspendPoint]) -> Self {
        let mut max_live = 0usize;

        for pt in points.iter() {
            if pt.live.len() > max_live {
                max_live = pt.live.len();
            }
        }

        let state_size = (1 + max_live) as u16;

        StateLayout { state_size }
    }
}
