use super::frame_store::FrameStore;
use crate::value::VmValue;
use varn_types::HeapRef;

impl FrameStore {
    
    

    
    #[inline(always)]
    pub fn dyn_live_top(&self) -> usize {
        self.dyn_.len()
    }

    
    #[inline(always)]
    pub fn ref_live_top(&self) -> usize {
        self.refs.len()
    }

    #[inline(always)]
    pub fn dyn_slice_mut(&mut self) -> &mut [VmValue] {
        &mut self.dyn_
    }

    #[inline(always)]
    pub fn ref_slice_mut(&mut self) -> &mut [Option<HeapRef>] {
        &mut self.refs
    }

    
    pub fn collect_roots(&self, top_dyn: usize, top_ref: usize, out: &mut Vec<HeapRef>) {
        out.extend(self.refs[..top_ref.min(self.refs.len())].iter().flatten());
        for v in &self.dyn_[..top_dyn.min(self.dyn_.len())] {
            if v.is_heap() {
                out.push(v.as_heap());
            }
        }
    }
}
