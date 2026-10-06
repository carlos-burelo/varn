


























use std::rc::Rc;

use varn_types::FunctionProto;

use crate::value::VmValue;




pub use varn_types::register_meta::SlotClass;







#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotAddr {
    pub class: SlotClass,
    pub idx: u32,
}





pub use varn_types::register_meta::FrameLayout;






#[repr(C)]
#[derive(Debug, Clone)]
pub struct FrameAlloc {
    pub bases: [u32; 4],
    pub layout: Rc<FrameLayout>,
}






#[repr(C)]
#[derive(Debug, Default)]
pub struct FrameStore {
    pub gpr: Vec<i64>,
    pub fpr: Vec<f64>,
    pub refs: Vec<Option<varn_types::HeapRef>>,
    pub dyn_: Vec<VmValue>,
    
    
    
    pub(crate) allocs: Vec<FrameAlloc>,
}

impl FrameStore {
    pub fn new() -> Self {
        Self {
            gpr: Vec::with_capacity(4096),
            fpr: Vec::with_capacity(1024),
            refs: Vec::with_capacity(2048),
            dyn_: Vec::with_capacity(8192),
            allocs: Vec::with_capacity(512),
        }
    }

    pub fn new_for_task() -> Self {
        Self {
            gpr: Vec::new(),
            fpr: Vec::new(),
            refs: Vec::new(),
            dyn_: Vec::new(),
            allocs: Vec::new(),
        }
    }

    
    pub fn push_frame(&mut self, proto: &Rc<FunctionProto>) -> usize {
        let layout = proto.frame_layout();
        let id = self.allocs.len();
        let mut bases = [0u32; 4];
        bases[SlotClass::Gpr.index()] = self.gpr.len() as u32;
        bases[SlotClass::Fpr.index()] = self.fpr.len() as u32;
        bases[SlotClass::Ref.index()] = self.refs.len() as u32;
        bases[SlotClass::Dyn.index()] = self.dyn_.len() as u32;
        self.gpr.extend(std::iter::repeat_n(
            0,
            layout.counts[SlotClass::Gpr.index()] as usize,
        ));
        self.fpr.extend(std::iter::repeat_n(
            0.0,
            layout.counts[SlotClass::Fpr.index()] as usize,
        ));
        self.refs.extend(std::iter::repeat_n(
            None,
            layout.counts[SlotClass::Ref.index()] as usize,
        ));
        self.dyn_.extend(std::iter::repeat_n(
            VmValue::null(),
            layout.counts[SlotClass::Dyn.index()] as usize,
        ));
        self.allocs.push(FrameAlloc { bases, layout });
        id
    }

    
    pub fn pop_frame(&mut self) {
        if let Some(alloc) = self.allocs.pop() {
            self.gpr
                .truncate(alloc.bases[SlotClass::Gpr.index()] as usize);
            self.fpr
                .truncate(alloc.bases[SlotClass::Fpr.index()] as usize);
            self.refs
                .truncate(alloc.bases[SlotClass::Ref.index()] as usize);
            self.dyn_
                .truncate(alloc.bases[SlotClass::Dyn.index()] as usize);
        }
    }

    
    
    pub fn ensure_frame_size(&mut self, id: usize, register_count: usize) {
        let (bases, counts) = {
            let a = &self.allocs[id];
            (a.bases, a.layout.counts)
        };
        let need = |base: u32, count: u32, len: usize| base as usize + count as usize > len;
        if need(bases[0], counts[0], self.gpr.len()) {
            self.gpr.resize(bases[0] as usize + counts[0] as usize, 0);
        }
        if need(bases[1], counts[1], self.fpr.len()) {
            self.fpr.resize(bases[1] as usize + counts[1] as usize, 0.0);
        }
        if need(bases[2], counts[2], self.refs.len()) {
            self.refs
                .resize(bases[2] as usize + counts[2] as usize, None);
        }
        if need(bases[3], counts[3], self.dyn_.len()) {
            self.dyn_
                .resize(bases[3] as usize + counts[3] as usize, VmValue::null());
        }
        let _ = register_count;
    }

    #[inline(always)]
    pub(crate) fn slot(&self, id: usize, reg: usize) -> (SlotClass, usize) {
        let a = &self.allocs[id];
        let (class, idx) = a.layout.slots[reg];
        (class, a.bases[class.index()] as usize + idx as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use varn_types::register_meta::{RegisterMeta, SlotKind};

    fn proto_with(meta: &[SlotKind], nregs: u16) -> Rc<FunctionProto> {
        Rc::new(FunctionProto {
            register_count: nregs,
            register_meta: meta.iter().map(|&kind| RegisterMeta { kind }).collect(),
            ..FunctionProto::default()
        })
    }

    #[test]
    fn layout_partitions_by_class() {
        use SlotKind as K;
        let p = proto_with(&[K::Dynamic, K::Int, K::Float, K::Int, K::Ref], 5);
        let l = FrameLayout::for_proto(&p);
        assert_eq!(l.counts, [2, 1, 1, 1]);
        assert_eq!(l.slots[0], (SlotClass::Dyn, 0));
        assert_eq!(l.slots[1], (SlotClass::Gpr, 0));
        assert_eq!(l.slots[2], (SlotClass::Fpr, 0));
        assert_eq!(l.slots[3], (SlotClass::Gpr, 1));
        assert_eq!(l.slots[4], (SlotClass::Ref, 0));
    }

    #[test]
    fn bool_and_str_stay_dynamic() {
        use SlotKind as K;
        let p = proto_with(&[K::Bool, K::Str], 2);
        let l = FrameLayout::for_proto(&p);
        assert_eq!(l.counts, [0, 0, 0, 2]);
    }

    #[test]
    fn push_pop_isolates_frames() {
        use SlotKind as K;
        let mut s = FrameStore::new();
        let p = proto_with(&[K::Int, K::Float], 2);
        let a = s.push_frame(&p);
        s.set_g(a, 0, 7);
        s.set_f(a, 1, 1.5);
        let b = s.push_frame(&p);
        assert_eq!(s.g(b, 0), 0);
        assert_eq!(s.f(b, 1), 0.0);
        s.set_g(b, 0, 9);
        assert_eq!(s.g(a, 0), 7);
        s.pop_frame();
        assert_eq!(s.g(a, 0), 7);
        assert_eq!(s.frame_count(), 1);
    }

    #[test]
    fn mov_converts_between_classes() {
        use SlotKind as K;
        let mut s = FrameStore::new();
        
        let p = proto_with(&[K::Dynamic, K::Int, K::Float, K::Ref], 4);
        let a = s.push_frame(&p);
        s.set_g(a, 1, 42);
        s.mov(a, 0, 1).unwrap();
        assert_eq!(s.d(a, 0), VmValue::from_int(42));
        
        s.mov(a, 1, 0).unwrap();
        assert_eq!(s.g(a, 1), 42);
        
        s.set_d(a, 0, VmValue::from_bool(true));
        assert!(s.mov(a, 1, 0).is_err());
        assert_eq!(s.g(a, 1), 42);
        
        s.set_f(a, 2, 2.5);
        s.mov(a, 0, 2).unwrap();
        assert_eq!(s.d(a, 0), VmValue::from_f64(2.5));
        s.mov(a, 2, 0).unwrap();
        assert_eq!(s.f(a, 2), 2.5);
    }

    #[test]
    fn uninit_ref_is_skipped_by_roots_and_reads_null() {
        use SlotKind as K;
        let mut s = FrameStore::new();
        let p = proto_with(&[K::Ref], 1);
        let a = s.push_frame(&p);
        let addr = s.addr_of(a, 0);
        assert_eq!(s.get_addr(addr), VmValue::null());
        let mut roots = Vec::new();
        s.collect_roots(0, 1, &mut roots);
        assert!(roots.is_empty());
    }
}
