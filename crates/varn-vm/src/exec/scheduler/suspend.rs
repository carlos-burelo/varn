use std::rc::Rc;
use std::sync::atomic::Ordering;

use crate::closure::{VmClosure, VmUpvalue};
use crate::exec::ctx::ExecCtx;
use crate::frame::{CallFrame, TryHandler};
use crate::frame_store::{FrameAlloc, SlotAddr};
use crate::value::VmValue;
use varn_types::register_meta::SlotClass;

pub(crate) struct FrozenFrame {
    closure: Rc<VmClosure>,
    ip: usize,
    return_reg: u16,
    awaited: bool,
    current_class: Option<Rc<varn_types::ClassObj>>,
    compact: bool,
    gpr_len: u32,
    plain: Vec<u64>,
    refs: Vec<Option<varn_types::HeapRef>>,
    dyn_: Vec<VmValue>,
    open: Vec<(SlotClass, u32, VmUpvalue)>,
    handlers: Vec<(usize, u8)>,
}

impl FrozenFrame {
    pub(crate) fn trace(&self, value: &mut dyn FnMut(VmValue)) {
        self.dyn_.iter().for_each(|&v| value(v));
        self.refs
            .iter()
            .flatten()
            .for_each(|&h| value(VmValue::from_heap(h)));
        for (_, _, uv) in &self.open {
            value(uv.inner.borrow().value);
        }
        for uv in &self.closure.upvalues {
            value(uv.inner.borrow().value);
        }
    }
}

#[derive(Default)]
pub(crate) struct FrameBuffers {
    plain: Vec<u64>,
    refs: Vec<Option<varn_types::HeapRef>>,
    dyn_: Vec<VmValue>,
}

pub(crate) struct Frozen {
    frames: Vec<FrozenFrame>,
    pub(crate) dest_reg: u16,
    jit_error_flag: u64,
    jit_native_result: VmValue,
}

impl Frozen {
    pub(crate) fn trace(&self, value: &mut dyn FnMut(VmValue)) {
        value(self.jit_native_result);
        self.frames.iter().for_each(|sf| sf.trace(value));
    }
}

fn live_regs(closure: &VmClosure, ip: usize) -> Option<&[u16]> {
    let ip = u32::try_from(ip).ok()?;
    let table = &closure.proto.suspend_live;
    let pos = table.binary_search_by_key(&ip, |e| e.resume_ip).ok()?;
    Some(&table[pos].regs)
}

fn keeps(
    regs: Option<&[u16]>,
    open: &[(SlotClass, u32, VmUpvalue)],
    reg: usize,
    class: SlotClass,
    idx: u32,
) -> bool {
    match regs {
        None => true,
        Some(regs) => {
            regs.binary_search(&(reg as u16)).is_ok()
                || open.iter().any(|(c, rel, _)| *c == class && *rel == idx)
        }
    }
}

pub(crate) fn freeze_frame(
    ctx: &mut ExecCtx,
    index: usize,
    compact: bool,
    buffers: FrameBuffers,
) -> Result<FrozenFrame, &'static str> {
    let frame = &ctx.frames[index];
    let closure = frame
        ._owned_closure
        .clone()
        .ok_or("frame does not own its closure")?;
    let alloc = frame.base;
    let ip = frame.ip;
    let return_reg = frame.return_reg;
    let awaited = frame.awaited;
    let current_class = frame.current_class.clone();
    let bases = ctx.stack.allocs[alloc].bases;
    let layout = Rc::clone(&ctx.stack.allocs[alloc].layout);
    let owns = |s: &SlotAddr| {
        let c = s.class.index();
        s.idx >= bases[c] && s.idx < bases[c] + layout.counts[c]
    };
    let mut open = Vec::new();
    for (s, uv) in &ctx.open_upvalues {
        if owns(s) {
            uv.close(&ctx.stack);
            open.push((s.class, s.idx - bases[s.class.index()], uv.clone()));
        }
    }
    ctx.open_upvalues.retain(|(s, _)| !owns(s));
    let depth = index + 1;
    let handlers = ctx
        .try_handlers
        .iter()
        .filter(|h| h.frame_depth == depth)
        .map(|h| (h.catch_ip, h.err_reg))
        .collect();
    ctx.try_handlers.retain(|h| h.frame_depth != depth);
    let regs = if compact {
        let found = live_regs(&closure, ip);
        let stat = match found {
            Some(_) => &crate::profile::TASK_STATS.prune_hit,
            None => &crate::profile::TASK_STATS.prune_miss,
        };
        stat.fetch_add(1, Ordering::Relaxed);
        found
    } else {
        None
    };
    let FrameBuffers {
        mut plain,
        mut refs,
        mut dyn_,
    } = buffers;
    let gpr_len = match regs {
        None => {
            let range = |c: usize| bases[c] as usize..(bases[c] + layout.counts[c]) as usize;
            plain.extend(ctx.stack.gpr[range(0)].iter().map(|&v| v as u64));
            let gpr_len = plain.len() as u32;
            plain.extend(ctx.stack.fpr[range(1)].iter().map(|v| v.to_bits()));
            refs.extend_from_slice(&ctx.stack.refs[range(2)]);
            dyn_.extend_from_slice(&ctx.stack.dyn_[range(3)]);
            gpr_len
        }
        Some(_) => {
            for (r, &(class, idx)) in layout.slots.iter().enumerate() {
                if class == SlotClass::Gpr && keeps(regs, &open, r, class, idx) {
                    plain.push(ctx.stack.gpr[(bases[0] + idx) as usize] as u64);
                }
            }
            let gpr_len = plain.len() as u32;
            for (r, &(class, idx)) in layout.slots.iter().enumerate() {
                if !keeps(regs, &open, r, class, idx) {
                    continue;
                }
                match class {
                    SlotClass::Gpr => {}
                    SlotClass::Fpr => {
                        plain.push(ctx.stack.fpr[(bases[1] + idx) as usize].to_bits())
                    }
                    SlotClass::Ref => refs.push(ctx.stack.refs[(bases[2] + idx) as usize]),
                    SlotClass::Dyn => dyn_.push(ctx.stack.dyn_[(bases[3] + idx) as usize]),
                }
            }
            gpr_len
        }
    };
    let compact = regs.is_some();
    Ok(FrozenFrame {
        closure,
        ip,
        return_reg,
        awaited,
        current_class,
        compact,
        gpr_len,
        plain,
        refs,
        dyn_,
        open,
        handlers,
    })
}

pub(crate) fn thaw_frame(ctx: &mut ExecCtx, sf: FrozenFrame) -> FrameBuffers {
    let id = ctx.stack.allocs.len();
    let bases = [
        ctx.stack.gpr.len() as u32,
        ctx.stack.fpr.len() as u32,
        ctx.stack.refs.len() as u32,
        ctx.stack.dyn_.len() as u32,
    ];
    let layout = sf.closure.proto.frame_layout();
    if sf.compact {
        ctx.stack
            .gpr
            .resize(ctx.stack.gpr.len() + layout.counts[0] as usize, 0);
        ctx.stack
            .fpr
            .resize(ctx.stack.fpr.len() + layout.counts[1] as usize, 0.0);
        ctx.stack
            .refs
            .resize(ctx.stack.refs.len() + layout.counts[2] as usize, None);
        ctx.stack.dyn_.resize(
            ctx.stack.dyn_.len() + layout.counts[3] as usize,
            VmValue::null(),
        );
        let regs = live_regs(&sf.closure, sf.ip);
        let (gpr_vals, fpr_vals) = sf.plain.split_at(sf.gpr_len as usize);
        let mut gpr_it = gpr_vals.iter();
        let mut fpr_it = fpr_vals.iter();
        let mut ref_it = sf.refs.iter();
        let mut dyn_it = sf.dyn_.iter();
        for (r, &(class, idx)) in layout.slots.iter().enumerate() {
            if !keeps(regs, &sf.open, r, class, idx) {
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
    } else {
        let (gpr_vals, fpr_vals) = sf.plain.split_at(sf.gpr_len as usize);
        ctx.stack.gpr.extend(gpr_vals.iter().map(|&v| v as i64));
        ctx.stack
            .fpr
            .extend(fpr_vals.iter().map(|&v| f64::from_bits(v)));
        ctx.stack.refs.extend_from_slice(&sf.refs);
        ctx.stack.dyn_.extend_from_slice(&sf.dyn_);
    }
    ctx.stack.allocs.push(FrameAlloc { bases, layout });
    for (class, rel, uv) in sf.open {
        let slot = SlotAddr {
            class,
            idx: bases[class.index()] + rel,
        };
        {
            let mut cell = uv.inner.borrow_mut();
            let _ = ctx.stack.set_addr(slot, cell.value);
            cell.stack_slot = Some(slot);
        }
        ctx.open_upvalues.push((slot, uv));
    }
    let mut frame = CallFrame::new_owned(sf.closure, id);
    frame.ip = sf.ip;
    frame.return_reg = sf.return_reg;
    frame.awaited = sf.awaited;
    frame.current_class = sf.current_class;
    ctx.frames.push(frame);
    let depth = ctx.frames.len();
    for (catch_ip, err_reg) in sf.handlers {
        ctx.try_handlers.push(TryHandler {
            catch_ip,
            frame_depth: depth,
            err_reg,
        });
    }
    let (mut plain, mut refs, mut dyn_) = (sf.plain, sf.refs, sf.dyn_);
    plain.clear();
    refs.clear();
    dyn_.clear();
    FrameBuffers { plain, refs, dyn_ }
}

fn check_contiguous(ctx: &ExecCtx) -> Result<(), &'static str> {
    let n = ctx.frames.len();
    if n == 0 || ctx.stack.allocs.len() != n {
        return Err("frame and allocation counts differ");
    }
    for (i, frame) in ctx.frames.iter().enumerate() {
        if frame.base != i {
            return Err("frame base is not its activation id");
        }
    }
    if ctx.stack.allocs[0].bases != [0, 0, 0, 0] {
        return Err("first activation does not start at zero");
    }
    Ok(())
}

pub(crate) fn freeze(ctx: &mut ExecCtx, dest_reg: u16) -> Result<Box<Frozen>, &'static str> {
    if !ctx.module_exports.is_empty()
        || !ctx.pending_constructors.is_empty()
        || !ctx.pending_setters.is_empty()
    {
        return Err("module or constructor state is live");
    }
    check_contiguous(ctx)?;
    let n = ctx.frames.len();
    let mut frames = Vec::with_capacity(n);
    for i in 0..n {
        frames.push(freeze_frame(ctx, i, i + 1 == n, FrameBuffers::default())?);
    }
    let jit_error_flag = std::mem::replace(&mut ctx.jit_error_flag, 0);
    let jit_native_result = std::mem::replace(&mut ctx.jit_native_result, VmValue::null());
    ctx.try_handlers.clear();
    ctx.open_upvalues.clear();
    ctx.stack.gpr.clear();
    ctx.stack.fpr.clear();
    ctx.stack.refs.clear();
    ctx.stack.dyn_.clear();
    ctx.stack.allocs.clear();
    ctx.frames.clear();
    Ok(Box::new(Frozen {
        frames,
        dest_reg,
        jit_error_flag,
        jit_native_result,
    }))
}

pub(crate) fn thaw(ctx: &mut ExecCtx, frozen: Frozen) {
    let Frozen {
        frames,
        jit_error_flag,
        jit_native_result,
        ..
    } = frozen;
    for sf in frames {
        thaw_frame(ctx, sf);
    }
    ctx.jit_error_flag = jit_error_flag;
    ctx.jit_native_result = jit_native_result;
}
