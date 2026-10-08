pub mod liveness;
pub mod regalloc_post;

use std::rc::Rc;
use std::time::Duration;
use varn_types::FunctionProto;

pub fn run_post_passes(proto: &mut FunctionProto, measure: bool) -> Duration {
    use varn_types::chunk::PoolEntry;

    let mut total = Duration::ZERO;
    for entry in proto.chunk.constants.iter_mut() {
        if let PoolEntry::Function(rc) = entry {
            match Rc::get_mut(rc) {
                Some(inner) => total += run_post_passes(inner, measure),
                None => {
                    let mut cloned = (**rc).clone();
                    total += run_post_passes(&mut cloned, measure);
                    *rc = Rc::new(cloned);
                }
            }
        }
    }

    total + regalloc_post::optimize_function(proto, measure)
}

#[cfg(test)]
#[path = "regalloc_post_tests.rs"]
mod regalloc_post_tests;
