use std::rc::Rc;
use std::sync::atomic::Ordering;

use crate::closure::{VmClosure, VmUpvalue};
use crate::exec::ctx::ExecCtx;
use crate::frame::{CallFrame, TryHandler};
use crate::frame_store::{FrameAlloc, SlotAddr, REF_UNINIT};
use crate::value::VmValue;
use varn_types::register_meta::SlotClass;

pub(crate) struct FrozenFrame {
    closure: Rc<VmClosure>,
    ip: usize,
    return_reg: u16,
    current_class: Option<Rc<varn_types::ClassObj>>,
    compact: bool,
    gpr_len: u32,
    plain: Vec<u64>,
    pub(crate) refs: Vec<u32>,
    pub(crate) dyn_: Vec<VmValue>,
}

pub(crate) struct Frozen {
    first: FrozenFrame,
    rest: Vec<FrozenFrame>,
    pub(crate) has_refs: bool,
    pub(crate) dest_reg: u16,
    try_handlers: Vec<TryHandler>,
    open_upvalues: Vec<(SlotAddr, VmUpvalue)>,
}

impl Frozen {
    pub(crate) fn frames(&self) -> impl Iterator<Item = &FrozenFrame> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }

    pub(crate) fn frame_mut(&mut self, index: usize) -> Option<&mut FrozenFrame> {
        match index {
            0 => Some(&mut self.first),
            n => self.rest.get_mut(n - 1),
        }
    }
}

struct Keep<'a> {
    regs: Option<&'a [u16]>,
    open: &'a [(SlotAddr, VmUpvalue)],
    bases: [u32; 4],
}

impl Keep<'_> {
    fn keeps(&self, reg: usize, class: SlotClass, idx: u32) -> bool {
        match self.regs {
            None => true,
            Some(regs) => {
                regs.binary_search(&(reg as u16)).is_ok()
                    || self.open.iter().any(|(slot, _)| {
                        slot.class == class && slot.idx == self.bases[class.index()] + idx
                    })
            }
        }
    }
}

fn live_regs(closure: &VmClosure, ip: usize) -> Option<&[u16]> {
    let ip = u32::try_from(ip).ok()?;
    let table = &closure.proto.suspend_live;
    let pos = table.binary_search_by_key(&ip, |e| e.resume_ip).ok()?;
    Some(&table[pos].regs)
}

fn check_contiguous(ctx: &ExecCtx) -> Result<Vec<Rc<VmClosure>>, &'static str> {
    let n = ctx.frames.len();
    if n == 0 || ctx.stack.allocs.len() != n {
        return Err("frame and allocation counts differ");
    }
    let lens = [
        ctx.stack.gpr.len(),
        ctx.stack.fpr.len(),
        ctx.stack.refs.len(),
        ctx.stack.dyn_.len(),
    ];
    let mut closures = Vec::with_capacity(n);
    for (i, frame) in ctx.frames.iter().enumerate() {
        if frame.base != i {
            return Err("frame base is not its activation id");
        }
        closures.push(
            frame
                ._owned_closure
                .clone()
                .ok_or("frame does not own its closure")?,
        );
        let alloc = &ctx.stack.allocs[i];
        let prev = if i == 0 {
            [0u32; 4]
        } else {
            ctx.stack.allocs[i - 1].bases
        };
        for c in 0..4 {
            if alloc.bases[c] < prev[c] || alloc.bases[c] as usize > lens[c] {
                return Err("activation bases are not contiguous");
            }
        }
    }
    if ctx.stack.allocs[0].bases != [0, 0, 0, 0] {
        return Err("first activation does not start at zero");
    }
    Ok(closures)
}

pub(crate) fn freeze(ctx: &mut ExecCtx, dest_reg: u16) -> Result<Box<Frozen>, &'static str> {
    if !ctx.module_exports.is_empty()
        || !ctx.pending_constructors.is_empty()
        || !ctx.pending_setters.is_empty()
    {
        return Err("module or constructor state is live");
    }
    let closures = check_contiguous(ctx)?;
    let n = closures.len();
    let mut first: Option<FrozenFrame> = None;
    let mut rest: Vec<FrozenFrame> = Vec::new();
    let mut has_refs = false;
    for (i, closure) in closures.into_iter().enumerate() {
        let frame = &ctx.frames[i];
        let regs = if i + 1 == n {
            let found = live_regs(&closure, frame.ip);
            let stat = match found {
                Some(_) => &crate::profile::TASK_STATS.prune_hit,
                None => &crate::profile::TASK_STATS.prune_miss,
            };
            stat.fetch_add(1, Ordering::Relaxed);
            found
        } else {
            None
        };
        let layout = Rc::clone(&ctx.stack.allocs[i].layout);
        let bases = ctx.stack.allocs[i].bases;
        let keep = Keep {
            regs,
            open: &ctx.open_upvalues,
            bases,
        };
        let mut plain: Vec<u64> = Vec::new();
        for (r, &(class, idx)) in layout.slots.iter().enumerate() {
            if class == SlotClass::Gpr && keep.keeps(r, class, idx) {
                plain.push(ctx.stack.gpr[(bases[0] + idx) as usize] as u64);
            }
        }
        let gpr_len = plain.len() as u32;
        let mut refs: Vec<u32> = Vec::new();
        let mut dyn_: Vec<VmValue> = Vec::new();
        for (r, &(class, idx)) in layout.slots.iter().enumerate() {
            if !keep.keeps(r, class, idx) {
                continue;
            }
            match class {
                SlotClass::Gpr => {}
                SlotClass::Fpr => plain.push(ctx.stack.fpr[(bases[1] + idx) as usize].to_bits()),
                SlotClass::Ref => refs.push(ctx.stack.refs[(bases[2] + idx) as usize]),
                SlotClass::Dyn => dyn_.push(ctx.stack.dyn_[(bases[3] + idx) as usize]),
            }
        }
        has_refs =
            has_refs || dyn_.iter().any(|v| v.is_heap()) || refs.iter().any(|&h| h != REF_UNINIT);
        let compact = regs.is_some();
        let frozen_frame = FrozenFrame {
            closure,
            ip: frame.ip,
            return_reg: frame.return_reg,
            current_class: frame.current_class.clone(),
            compact,
            gpr_len,
            plain,
            refs,
            dyn_,
        };
        match first {
            None => first = Some(frozen_frame),
            Some(_) => rest.push(frozen_frame),
        }
    }
    let try_handlers = std::mem::take(&mut ctx.try_handlers);
    let open_upvalues = std::mem::take(&mut ctx.open_upvalues);
    ctx.stack.gpr.clear();
    ctx.stack.fpr.clear();
    ctx.stack.refs.clear();
    ctx.stack.dyn_.clear();
    ctx.stack.allocs.clear();
    ctx.frames.clear();
    let first = first.ok_or("no frames to freeze")?;
    Ok(Box::new(Frozen {
        first,
        rest,
        has_refs,
        dest_reg,
        try_handlers,
        open_upvalues,
    }))
}

pub(crate) fn thaw(ctx: &mut ExecCtx, frozen: Frozen) {
    let Frozen {
        first,
        rest,
        try_handlers,
        open_upvalues,
        ..
    } = frozen;
    for sf in std::iter::once(first).chain(rest) {
        let id = ctx.stack.allocs.len();
        let bases = [
            ctx.stack.gpr.len() as u32,
            ctx.stack.fpr.len() as u32,
            ctx.stack.refs.len() as u32,
            ctx.stack.dyn_.len() as u32,
        ];
        let layout = sf.closure.proto.frame_layout();
        ctx.stack
            .gpr
            .resize(ctx.stack.gpr.len() + layout.counts[0] as usize, 0);
        ctx.stack
            .fpr
            .resize(ctx.stack.fpr.len() + layout.counts[1] as usize, 0.0);
        ctx.stack
            .refs
            .resize(ctx.stack.refs.len() + layout.counts[2] as usize, REF_UNINIT);
        ctx.stack.dyn_.resize(
            ctx.stack.dyn_.len() + layout.counts[3] as usize,
            VmValue::null(),
        );
        let regs = if sf.compact {
            live_regs(&sf.closure, sf.ip)
        } else {
            None
        };
        let keep = Keep {
            regs,
            open: &open_upvalues,
            bases,
        };
        let (gpr_vals, fpr_vals) = sf.plain.split_at(sf.gpr_len as usize);
        let mut gpr_it = gpr_vals.iter();
        let mut fpr_it = fpr_vals.iter();
        let mut ref_it = sf.refs.iter();
        let mut dyn_it = sf.dyn_.iter();
        for (r, &(class, idx)) in layout.slots.iter().enumerate() {
            if !keep.keeps(r, class, idx) {
                continue;
            }
            let at = (bases[class.index()] + idx) as usize;
            match class {
                SlotClass::Gpr => {
                    if let Some(&v) = gpr_it.next() {
                        ctx.stack.gpr[at] = v as i64;
                    }
                }
                SlotClass::Fpr => {
                    if let Some(&v) = fpr_it.next() {
                        ctx.stack.fpr[at] = f64::from_bits(v);
                    }
                }
                SlotClass::Ref => {
                    if let Some(&v) = ref_it.next() {
                        ctx.stack.refs[at] = v;
                    }
                }
                SlotClass::Dyn => {
                    if let Some(&v) = dyn_it.next() {
                        ctx.stack.dyn_[at] = v;
                    }
                }
            }
        }
        ctx.stack.allocs.push(FrameAlloc { bases, layout });
        let mut frame = CallFrame::new_owned(sf.closure, id);
        frame.ip = sf.ip;
        frame.return_reg = sf.return_reg;
        frame.current_class = sf.current_class;
        ctx.frames.push(frame);
    }
    ctx.try_handlers = try_handlers;
    ctx.open_upvalues = open_upvalues;
}
