use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};

pub type Handle = i64;

struct Slot {
    epoch: u32,
    len: usize,
    dims: Vec<usize>,
    data: Option<Vec<f32>>,
    node: Option<NodeRef>,
}

struct NodeRef {
    op: crate::Op,
    inputs: Vec<Handle>,
    len: usize,
}

pub struct Bufs {
    slots: Vec<Option<Slot>>,
    free: Vec<u32>,
    next_gen: u32,
}

impl Bufs {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            next_gen: 1,
        }
    }

    fn pack(idx: u32, epoch: u32) -> Handle {
        ((epoch as i64) << 32) | (idx as i64)
    }

    fn unpack(h: Handle) -> Option<(u32, u32)> {
        if h <= 0 {
            return None;
        }
        Some(((h & 0xffff_ffff) as u32, (h >> 32) as u32))
    }

    fn get(&self, h: Handle) -> Result<(u32, &Slot), String> {
        let (idx, epoch) = Self::unpack(h).ok_or_else(|| "gpu: bad handle".to_string())?;
        let s = self
            .slots
            .get(idx as usize)
            .and_then(|s| s.as_ref())
            .ok_or_else(|| "gpu: destroyed handle".to_string())?;
        if s.epoch != epoch {
            return Err("gpu: stale handle".to_string());
        }
        Ok((idx, s))
    }

    fn get_mut(&mut self, h: Handle) -> Result<(u32, &mut Slot), String> {
        let (idx, epoch) = Self::unpack(h).ok_or_else(|| "gpu: bad handle".to_string())?;
        let s = self
            .slots
            .get_mut(idx as usize)
            .and_then(|s| s.as_mut())
            .ok_or_else(|| "gpu: destroyed handle".to_string())?;
        if s.epoch != epoch {
            return Err("gpu: stale handle".to_string());
        }
        Ok((idx, s))
    }

    pub fn create(&mut self, len: usize) -> Handle {
        let epoch = self.next_gen;
        self.next_gen = self.next_gen.wrapping_add(1).max(1);
        let slot = Slot {
            epoch,
            len,
            dims: vec![len],
            data: Some(vec![0.0; len]),
            node: None,
        };
        if let Some(idx) = self.free.pop() {
            self.slots[idx as usize] = Some(slot);
            Self::pack(idx, epoch)
        } else {
            let idx = self.slots.len() as u32;
            self.slots.push(Some(slot));
            Self::pack(idx, epoch)
        }
    }

    pub fn upload(&mut self, vals: Vec<f32>) -> Handle {
        let epoch = self.next_gen;
        self.next_gen = self.next_gen.wrapping_add(1).max(1);
        let len = vals.len();
        let slot = Slot {
            epoch,
            len,
            dims: vec![len],
            data: Some(vals),
            node: None,
        };
        if let Some(idx) = self.free.pop() {
            self.slots[idx as usize] = Some(slot);
            Self::pack(idx, epoch)
        } else {
            let idx = self.slots.len() as u32;
            self.slots.push(Some(slot));
            Self::pack(idx, epoch)
        }
    }

    pub fn lazy(
        &mut self,
        op: crate::Op,
        inputs: Vec<Handle>,
        len: usize,
        dims: Vec<usize>,
    ) -> Result<Handle, String> {
        for h in &inputs {
            self.get(*h)?;
        }
        let epoch = self.next_gen;
        self.next_gen = self.next_gen.wrapping_add(1).max(1);
        let slot = Slot {
            epoch,
            len,
            dims,
            data: None,
            node: Some(NodeRef { op, inputs, len }),
        };
        if let Some(idx) = self.free.pop() {
            self.slots[idx as usize] = Some(slot);
            Ok(Self::pack(idx, epoch))
        } else {
            let idx = self.slots.len() as u32;
            self.slots.push(Some(slot));
            Ok(Self::pack(idx, epoch))
        }
    }

    pub fn destroy(&mut self, h: Handle) -> Result<(), String> {
        let (idx, _) = self.get(h)?;
        self.slots[idx as usize] = None;
        self.free.push(idx);
        Ok(())
    }

    pub fn len(&self, h: Handle) -> Result<usize, String> {
        Ok(self.get(h)?.1.len)
    }

    pub fn dims(&self, h: Handle) -> Result<Vec<usize>, String> {
        Ok(self.get(h)?.1.dims.clone())
    }

    pub fn reshape(&mut self, h: Handle, dims: Vec<usize>) -> Result<(), String> {
        let len = self.get(h)?.1.len;
        if dims.iter().product::<usize>() != len {
            return Err(format!("gpu: reshape {:?} != len {len}", dims));
        }
        self.get_mut(h)?.1.dims = dims;
        Ok(())
    }

    pub fn is_pending(&self, h: Handle) -> Result<bool, String> {
        Ok(self.get(h)?.1.data.is_none())
    }

    pub fn resolve(&mut self, h: Handle) -> Result<Vec<f32>, String> {
        let mut stack = vec![h];
        let mut order = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(cur) = stack.pop() {
            if !seen.insert(cur) {
                continue;
            }
            let (_, s) = self.get(cur)?;
            if s.data.is_some() {
                continue;
            }
            let node = s
                .node
                .as_ref()
                .ok_or_else(|| "gpu: corrupt slot".to_string())?;
            order.push(cur);
            for inp in &node.inputs {
                stack.push(*inp);
            }
        }
        order.reverse();
        if self.try_gpu(h, &order).unwrap_or(false) {
            return self.resolve_cached(h);
        }
        crate::mark_cpu();
        for cur in order {
            if self.get(cur)?.1.data.is_some() {
                continue;
            }
            let (op, inputs, len) = {
                let (_, s) = self.get(cur)?;
                let n = s
                    .node
                    .as_ref()
                    .ok_or_else(|| "gpu: corrupt slot".to_string())?;
                (n.op, n.inputs.clone(), n.len)
            };
            let mut ins: Vec<Vec<f32>> = Vec::with_capacity(inputs.len());
            for inp in &inputs {
                ins.push(self.resolve_cached(*inp)?);
            }
            let data = crate::graph::run_op(op, &ins, &inputs, self, len)?;
            let (_, s) = self.get_mut(cur)?;
            s.data = Some(data);
            s.node = None;
        }
        self.resolve_cached(h)
    }

    fn try_gpu(&mut self, root: Handle, order: &[Handle]) -> Result<bool, String> {
        let mut jobs = Vec::with_capacity(order.len());
        let mut macs = 0usize;
        for cur in order {
            let (_, s) = self.get(*cur)?;
            let n = s
                .node
                .as_ref()
                .ok_or_else(|| "gpu: corrupt slot".to_string())?;
            macs += match n.op {
                crate::Op::MatMul { m, k, n: nn } => m * k * nn,
                crate::Op::Add | crate::Op::Mul | crate::Op::Scale { .. } => s.len,
            };
            jobs.push(crate::execute::Job {
                handle: *cur,
                op: n.op,
                inputs: n.inputs.clone(),
                len: s.len,
            });
        }
        if macs < crate::GPU_MIN_MACS {
            return Ok(false);
        }
        let Some(c) = crate::execute::ctx() else {
            return Ok(false);
        };
        let mut leaves = std::collections::HashMap::new();
        let mut stack: Vec<Handle> = vec![root];
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(cur) = stack.pop() {
            if !seen.insert(cur) {
                continue;
            }
            let (_, s) = self.get(cur)?;
            if let Some(d) = &s.data {
                leaves.insert(cur, d.clone());
                continue;
            }
            let n = s
                .node
                .as_ref()
                .ok_or_else(|| "gpu: corrupt slot".to_string())?;
            stack.extend(n.inputs.iter().copied());
        }
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::execute::run(c, &jobs, &leaves)
        }));
        let Ok(Ok(map)) = out else {
            return Ok(false);
        };
        for (hh, data) in map {
            let (_, s) = self.get_mut(hh)?;
            s.data = Some(data);
            s.node = None;
        }
        crate::mark_gpu();
        Ok(true)
    }

    fn resolve_cached(&mut self, h: Handle) -> Result<Vec<f32>, String> {
        let (_, s) = self.get(h)?;
        if let Some(d) = &s.data {
            return Ok(d.clone());
        }
        Err("gpu: unresolved".to_string())
    }

    pub fn data(&mut self, h: Handle) -> Result<Vec<f32>, String> {
        self.resolve(h)
    }
}

static POOL: OnceLock<Arc<Mutex<Bufs>>> = OnceLock::new();

pub fn pool() -> Arc<Mutex<Bufs>> {
    POOL.get_or_init(|| Arc::new(Mutex::new(Bufs::new())))
        .clone()
}
