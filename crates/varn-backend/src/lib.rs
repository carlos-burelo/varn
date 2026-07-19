pub mod ir;
pub mod liveness;
pub mod regalloc_post;

use std::rc::Rc;
use varn_types::FunctionProto;

pub fn run_post_passes(proto: &mut FunctionProto) {
    use varn_types::chunk::PoolEntry;

    for entry in proto.chunk.constants.iter_mut() {
        if let PoolEntry::Function(rc) = entry {
            match Rc::get_mut(rc) {
                Some(inner) => run_post_passes(inner),
                None => {
                    let mut cloned = (**rc).clone();
                    run_post_passes(&mut cloned);
                    *rc = Rc::new(cloned);
                }
            }
        }
    }

    regalloc_post::optimize_function(proto);
}
