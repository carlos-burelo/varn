

































pub mod layout;
pub mod transform;

use crate::ssa::ir::SsaFunc;
use crate::ssa::suspend;



pub fn run(func: &mut SsaFunc) -> u16 {
    let is_suspendible = func.is_async || func.is_generator;
    if !is_suspendible {
        return 0;
    }

    let points = suspend::analyze(func);
    if points.is_empty() {
        
        
        
        return 1;
    }

    transform::transform_suspend_func(func, &points)
}
