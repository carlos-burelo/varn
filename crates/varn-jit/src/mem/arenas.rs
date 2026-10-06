use super::buffer::JitBuffer;
use core::mem::size_of;

pub const STACK_RESERVE_BYTES: usize = 64 << 20;

pub struct StackArenas {
    gpr: JitBuffer,
    fpr: JitBuffer,
    refs: JitBuffer,
    dyn_: JitBuffer,
    top_gpr: u32,
    top_fpr: u32,
    top_ref: u32,
    top_dyn: u32,
}

impl StackArenas {
    pub fn new(bytes: usize) -> Result<Self, String> {
        Ok(Self {
            gpr: JitBuffer::new(bytes)?,
            fpr: JitBuffer::new(bytes)?,
            refs: JitBuffer::new(bytes)?,
            dyn_: JitBuffer::new(bytes)?,
            top_gpr: 0,
            top_fpr: 0,
            top_ref: 0,
            top_dyn: 0,
        })
    }

    pub fn abi_stacks(&self) -> varn_abi::AbiStacks {
        let gpr = self.gpr.as_ptr() as *mut i64;
        let fpr = self.fpr.as_ptr() as *mut f64;
        let refs = self.refs.as_ptr() as *mut u32;
        let dyn_ = self.dyn_.as_ptr() as *mut varn_abi::AbiValue;

        unsafe {
            varn_abi::AbiStacks {
                gpr,
                gpr_end: gpr.add(self.gpr.size() / size_of::<i64>()),
                fpr,
                fpr_end: fpr.add(self.fpr.size() / size_of::<f64>()),
                refs,
                refs_end: refs.add(self.refs.size() / size_of::<u32>()),
                dyn_,
                dyn_end: dyn_.add(self.dyn_.size() / size_of::<varn_abi::AbiValue>()),
            }
        }
    }

    pub fn alloc(&mut self, counts: [u32; 4]) -> Option<varn_abi::ActBases> {
        let cap = |bytes: usize, elem: usize| (bytes / elem) as u64;
        let ok = (self.top_gpr as u64 + counts[0] as u64) <= cap(self.gpr.size(), 8)
            && (self.top_fpr as u64 + counts[1] as u64) <= cap(self.fpr.size(), 8)
            && (self.top_ref as u64 + counts[2] as u64) <= cap(self.refs.size(), 4)
            && (self.top_dyn as u64 + counts[3] as u64) <= cap(self.dyn_.size(), 16);
        if !ok {
            return None;
        }
        let bases = varn_abi::ActBases {
            bases: [self.top_gpr, self.top_fpr, self.top_ref, self.top_dyn],
        };
        self.top_gpr += counts[0];
        self.top_fpr += counts[1];
        self.top_ref += counts[2];
        self.top_dyn += counts[3];
        Some(bases)
    }

    pub fn truncate(&mut self, bases: varn_abi::ActBases) {
        self.top_gpr = bases.bases[0];
        self.top_fpr = bases.bases[1];
        self.top_ref = bases.bases[2];
        self.top_dyn = bases.bases[3];
    }

    pub fn reset(&mut self) {
        self.top_gpr = 0;
        self.top_fpr = 0;
        self.top_ref = 0;
        self.top_dyn = 0;
    }
}

pub struct FrameArena {
    buf: JitBuffer,
    len: u32,
}

impl FrameArena {
    pub fn new(cap_frames: u32) -> Result<Self, String> {
        let bytes = (cap_frames as usize).saturating_mul(size_of::<varn_abi::AbiFrame>());
        Ok(Self {
            buf: JitBuffer::new(bytes.max(4096))?,
            len: 0,
        })
    }

    pub fn cap(&self) -> u32 {
        (self.buf.size() / size_of::<varn_abi::AbiFrame>()) as u32
    }

    pub fn push(&mut self, frame: varn_abi::AbiFrame) -> Option<u32> {
        if self.len >= self.cap() {
            return None;
        }
        let id = self.len;

        unsafe {
            let base = self.buf.as_ptr() as *mut varn_abi::AbiFrame;
            base.add(id as usize).write(frame);
        }
        self.len += 1;
        Some(id)
    }

    pub fn pop(&mut self) {
        self.len = self.len.saturating_sub(1);
    }

    pub fn abi(&self) -> varn_abi::AbiFrameArena {
        varn_abi::AbiFrameArena {
            base: self.buf.as_ptr() as *mut varn_abi::AbiFrame,
            len: self.len,
            cap: self.cap(),
        }
    }
}

#[cfg(test)]
mod abi_v2_tests {
    use super::*;

    #[test]
    fn bases_estables_tras_alloc() {
        let mut a = StackArenas::new(1 << 20).unwrap();
        let b0 = a.abi_stacks();
        a.alloc([10, 5, 5, 5]).unwrap();
        let b1 = a.abi_stacks();
        assert_eq!(b0.gpr, b1.gpr);
        assert_eq!(b0.fpr, b1.fpr);
        assert_eq!(b0.refs, b1.refs);
        assert_eq!(b0.dyn_ as *const u8, b1.dyn_ as *const u8);
    }

    #[test]
    fn overflow_rechaza_slow_path() {
        let mut a = StackArenas::new(4096).unwrap();
        assert!(a.alloc([u32::MAX, 0, 0, 0]).is_none());
    }

    #[test]
    fn frame_arena_push_pop() {
        let mut f = FrameArena::new(4).unwrap();
        let frame = varn_abi::AbiFrame {
            closure: core::ptr::null(),
            caller: 0,
            resume: 0,
            dest: 0,
            _pad: 0,
            bases: varn_abi::ActBases { bases: [0; 4] },
        };
        assert_eq!(f.push(frame), Some(0));
        assert_eq!(f.push(frame), Some(1));
        f.pop();
        assert_eq!(f.abi().len, 1);
    }
}
