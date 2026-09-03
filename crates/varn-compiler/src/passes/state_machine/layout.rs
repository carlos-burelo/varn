//! Deterministic layout of state structures for state machine transformation.
//!
//! Maps each suspension point to its resume discriminant (`FIRST_RESUME + k`)
//! and maps each live value crossing that suspension point (`live_after`) to a
//! state object slot (`1..`).
//!
//! Calculates the total `state_size` in words (1 word for discriminant + max live slots).

use crate::ssa::ir::Value;
use crate::ssa::suspend::SuspendPoint;
use varn_types::FIRST_RESUME;

#[derive(Debug, Clone)]
pub struct PointLayout {
    /// Resume discriminant for this suspension point (`FIRST_RESUME + k`).
    pub resume_disc: u32,
    /// Live values crossing this suspension point, mapped to their slot index.
    /// Slot 0 is reserved for the discriminant (`state[0]`).
    /// Slots 1.. are used for live variables.
    pub slots: Vec<(Value, u16)>,
}

#[derive(Debug, Clone)]
pub struct StateLayout {
    /// Layout for each suspension point, in point index order.
    pub points: Vec<PointLayout>,
    /// Total words occupied by the state object (`1 + max_live_slots`).
    pub state_size: u16,
}

impl StateLayout {
    pub fn compute(points: &[SuspendPoint]) -> Self {
        let mut point_layouts = Vec::with_capacity(points.len());
        let mut max_live = 0usize;

        for (k, pt) in points.iter().enumerate() {
            let resume_disc = FIRST_RESUME + k as u32;
            let mut slots = Vec::with_capacity(pt.live.len());

            for (slot_idx, &val) in pt.live.iter().enumerate() {
                // Slot 0 is the discriminant; live values start at slot 1.
                let slot = (1 + slot_idx) as u16;
                slots.push((val, slot));
            }

            if slots.len() > max_live {
                max_live = slots.len();
            }

            point_layouts.push(PointLayout { resume_disc, slots });
        }

        let state_size = (1 + max_live) as u16;

        StateLayout {
            points: point_layouts,
            state_size,
        }
    }
}
