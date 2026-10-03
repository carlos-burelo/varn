use super::ctx::ExecCtx;
use super::VmSuspend;
use crate::value::VmValue;
use std::rc::Rc;

struct MinorSeg {
    owner: usize,
    kind: u8,
    start: usize,
    len: usize,
    aux: usize,
}

struct MinorRefSeg {
    owner: usize,
    start: usize,
    len: usize,
}

struct StashSeg {
    sowner: usize,
    frame: usize,
    is_refs: bool,
    src: usize,
    len: usize,
}

fn gather_minor_roots(
    ctx: &ExecCtx,
    owner: usize,
    vals: &mut Vec<VmValue>,
    refs: &mut Vec<u32>,
    segs: &mut Vec<MinorSeg>,
    refsegs: &mut Vec<MinorRefSeg>,
) {
    macro_rules! seg {
        ($kind:expr, $aux:expr, $body:block) => {{
            let start = vals.len();
            $body
            segs.push(MinorSeg {
                owner,
                kind: $kind,
                start,
                len: vals.len() - start,
                aux: $aux,
            });
        }};
    }
    seg!(0, 0, {
        // El almacén por clases dimensiona exacto por activación
        // (`push_frame` extiende, `pop_frame` trunca): los tramos vivos son
        // contiguos desde 0 y GPR/FPR ni se visitan — por construcción nunca
        // son raíces. Solo DYN se filtra por tag y REF va directo.
        vals.extend_from_slice(&ctx.stack.dyn_);
    });
    seg!(1, 0, {
        // Call-window staging: boxed args/receiver/result sitting here
        // between `exec_call_reg`'s slow path staging them and
        // `prepare_call`/`dispatch_prepared_call` consuming them are live
        // the same way any other pending value is — a heap ref parked here
        // when a nested allocation trips this same safepoint (e.g. building
        // a rest-args array) must survive nursery collection like everything
        // else, not just what already made it into a register.
        vals.extend_from_slice(&ctx.stage);
    });
    seg!(2, 0, {
        if owner == 0 {
            vals.extend_from_slice(&ctx.globals_ref().values);
        }
    });
    seg!(3, 0, {
        if owner == 0 {
            for v in ctx.modules_ref().values() {
                vals.push(*v);
            }
        }
    });
    seg!(4, 0, {
        for v in ctx.module_exports.values() {
            vals.push(*v);
        }
    });
    seg!(5, 0, {
        if owner == 0 {
            for (_, v) in unsafe { &*ctx.static_closures.get() }.values() {
                vals.push(*v);
            }
        }
    });
    if owner == 0 {
        for (key, (_, pool)) in unsafe { &*ctx.proto_constants.get() }.iter() {
            seg!(6, *key, {
                vals.extend_from_slice(pool);
            });
        }
    }
    seg!(7, 0, {
        for (_, v) in &ctx.pending_constructors {
            vals.push(*v);
        }
    });
    seg!(8, 0, {
        for (_, v) in &ctx.pending_setters {
            vals.push(*v);
        }
    });
    seg!(9, 0, {
        if let Some(VmSuspend::Yield { value, .. } | VmSuspend::Await { value, .. }) =
            &ctx.vm_suspend
        {
            vals.push(*value);
        }
    });
    seg!(10, 0, {
        if owner == 0 {
            for map in unsafe { &*ctx.metadata.get() }.values() {
                for &v in map.values() {
                    vals.push(v);
                }
            }
        }
    });
    seg!(11, 0, {
        if owner == 0 {
            for reps in unsafe { &*ctx.hashable_keys.get() }.values() {
                vals.extend_from_slice(reps);
            }
        }
    });
    seg!(12, 0, {
        vals.push(ctx.jit_native_result);
    });
    let ref_start = refs.len();
    refs.extend_from_slice(&ctx.stack.refs);
    refsegs.push(MinorRefSeg {
        owner,
        start: ref_start,
        len: ctx.stack.refs.len(),
    });
}

fn write_minor_seg(ctx: &mut ExecCtx, seg: &MinorSeg, vals: &[VmValue]) {
    let slice = &vals[seg.start..seg.start + seg.len];
    match seg.kind {
        0 => ctx.stack.dyn_.copy_from_slice(slice),
        1 => ctx.stage.copy_from_slice(slice),
        2 => ctx.globals_mut().values.copy_from_slice(slice),
        3 => {
            for (v, w) in unsafe { &mut *ctx.modules.get() }
                .values_mut()
                .zip(slice.iter())
            {
                *v = *w;
            }
        }
        4 => {
            for (v, w) in ctx.module_exports.values_mut().zip(slice.iter()) {
                *v = *w;
            }
        }
        5 => {
            for ((_, v), w) in unsafe { &mut *ctx.static_closures.get() }
                .values_mut()
                .zip(slice.iter())
            {
                *v = *w;
            }
        }
        6 => {
            if let Some((_, pool)) = unsafe { &mut *ctx.proto_constants.get() }.get_mut(&seg.aux) {
                Rc::make_mut(pool).copy_from_slice(slice);
            }
        }
        7 => {
            for ((_, v), w) in ctx.pending_constructors.iter_mut().zip(slice.iter()) {
                *v = *w;
            }
        }
        8 => {
            for ((_, v), w) in ctx.pending_setters.iter_mut().zip(slice.iter()) {
                *v = *w;
            }
        }
        9 => {
            if seg.len == 1 {
                if let Some(VmSuspend::Yield { value, .. } | VmSuspend::Await { value, .. }) =
                    &mut ctx.vm_suspend
                {
                    *value = slice[0];
                }
            }
        }
        10 => {
            let mut idx = 0;
            for map in unsafe { &mut *ctx.metadata.get() }.values_mut() {
                for v in map.values_mut() {
                    *v = slice[idx];
                    idx += 1;
                }
            }
        }
        11 => {
            let mut idx = 0;
            for reps in unsafe { &mut *ctx.hashable_keys.get() }.values_mut() {
                for v in reps.iter_mut() {
                    *v = slice[idx];
                    idx += 1;
                }
            }
        }
        _ => {
            ctx.jit_native_result = slice[0];
        }
    }
}

impl ExecCtx {
    pub fn run_minor_gc(&mut self) {
        // Union single-pass collection over this context plus every queued
        // fork. A per-fork collection would wipe the nursery before the
        // driver's own roots are updated, leaving them dangling (evacuate
        // fallback); gathering every root set first and collecting once
        // keeps one coherent forwarding table for all owners.
        let scope = super::scheduler::gc_scope(self);
        let mut owners = scope.owners;

        let mut all_vals = std::mem::take(&mut self.gc_root_scratch);
        all_vals.clear();
        let mut all_refs: Vec<u32> = Vec::new();
        let mut segs: Vec<MinorSeg> = Vec::new();
        let mut refsegs: Vec<MinorRefSeg> = Vec::new();
        let mut stash_segs: Vec<StashSeg> = Vec::new();
        for (owner_idx, owner_ptr) in owners.iter().enumerate() {
            // Shared borrow only; no mutation happens during gather, and no
            // collection runs until every owner contributed its roots.
            let ctx: &ExecCtx = unsafe { &**owner_ptr };
            gather_minor_roots(
                ctx,
                owner_idx,
                &mut all_vals,
                &mut all_refs,
                &mut segs,
                &mut refsegs,
            );
        }
        let mut sowners = scope.frozen;
        for (si, ptr) in sowners.iter().enumerate() {
            let st: &super::scheduler::Frozen = unsafe { &**ptr };
            if !st.has_refs {
                continue;
            }
            for (fi, sf) in st.frames().enumerate() {
                let start = all_vals.len();
                all_vals.extend_from_slice(&sf.dyn_);
                stash_segs.push(StashSeg {
                    sowner: si,
                    frame: fi,
                    is_refs: false,
                    src: start,
                    len: sf.dyn_.len(),
                });
                let rstart = all_refs.len();
                all_refs.extend_from_slice(&sf.refs);
                stash_segs.push(StashSeg {
                    sowner: si,
                    frame: fi,
                    is_refs: true,
                    src: rstart,
                    len: sf.refs.len(),
                });
            }
        }

        // TODO EL tramo, no solo `dyn_`: `all_vals` junta stack+stage+globals+
        // módulos+... precisamente para que cada uno cuente como raíz. Pasar
        // solo `[..dyn_len]` (como hacía esto) escaneaba los registros y
        // dejaba globals/static_closures/etc. sin tocar — el nursery los
        // wipea igual (`objects.clear()` es incondicional al final de
        // `collect`), así que cualquier objeto SOLO alcanzable desde un
        // global sobrevivía en el papel (la copia de vuelta no cambiaba nada)
        // pero desaparecía del heap: el primer `heap.get` posterior a un
        // minor GC con ese índice devolvía `None` — "invalid heap index" en
        // la siguiente llamada a una función de nivel de módulo.
        self.heap
            .minor_gc(&mut all_vals[..], &mut all_refs[..], &[]);

        for seg in segs.iter() {
            if seg.len == 0 {
                continue;
            }
            // Each owner is written back sequentially and exclusively; no
            // borrows overlap because only one `&mut` exists at a time.
            let ctx: &mut ExecCtx = unsafe { &mut *owners[seg.owner] };
            write_minor_seg(ctx, seg, &all_vals);
        }
        for refseg in refsegs.iter() {
            let ctx: &mut ExecCtx = unsafe { &mut *owners[refseg.owner] };
            let dst = &mut ctx.stack.refs[..refseg.len];
            dst.copy_from_slice(&all_refs[refseg.start..refseg.start + refseg.len]);
        }
        for sseg in stash_segs.iter() {
            let st: &mut super::scheduler::Frozen = unsafe { &mut *sowners[sseg.sowner] };
            if let Some(sf) = st.frame_mut(sseg.frame) {
                if sseg.is_refs {
                    if sf.refs.len() == sseg.len {
                        sf.refs
                            .copy_from_slice(&all_refs[sseg.src..sseg.src + sseg.len]);
                    }
                } else if sf.dyn_.len() == sseg.len {
                    sf.dyn_
                        .copy_from_slice(&all_vals[sseg.src..sseg.src + sseg.len]);
                }
            }
        }
        all_vals.clear();
        self.gc_root_scratch = all_vals;
    }
}
