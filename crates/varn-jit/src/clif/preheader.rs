//! Loop preheader: resolving a hoisted array receiver once, before the loop.
//!
//! `scan::loop_regions` decides WHICH receivers are hoistable; this emits the
//! resolve. It runs in the fall-through block ahead of a loop header, so the
//! accesses inside the body can test a cached pointer instead of re-walking
//! tag → generation → slot → payload on every iteration.
//!
//! `0` is the "not resolved" sentinel — no live allocation sits at address 0,
//! and it is the same sentinel the back-edge safepoint resets the caches to
//! after a collection.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use std::collections::HashMap;

use super::emit::{self, emit_array_payload, RegionCache};
use super::kinds::K;
use crate::JitHelpers;

/// Resolve every planned receiver of the region(s) whose header is `ip`.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_region_caches(
    b: &mut FunctionBuilder,
    helpers: &JitHelpers,
    exec_ctx: cranelift_codegen::ir::Value,
    vars: &[cranelift_frontend::Variable],
    cache_vars: &HashMap<(usize, usize), RegionCache>,
    regions: &[emit::Region],
    state: &[K],
    ip: usize,
) {
    for (h, _, regs, _) in regions.iter().filter(|(h, _, _, _)| *h == ip) {
        for &r in regs {
            if state[r] != K::Boxed {
                continue;
            }
            let obj = b.use_var(vars[r]);
            let cache = cache_vars[&(*h, r)];
            let invalid = b.create_block();
            let done = b.create_block();
            b.append_block_param(done, types::I64);
            let payload = emit_array_payload(
                b,
                exec_ctx,
                obj,
                &helpers.array_layout,
                helpers.heap_field_offset,
                invalid,
                false,
                true,
            );
            b.ins().jump(done, &[payload.into()]);
            b.switch_to_block(invalid);
            let z = b.ins().iconst(types::I64, 0);
            b.ins().jump(done, &[z.into()]);
            b.switch_to_block(done);
            let resolved = b.block_params(done)[0];
            b.def_var(cache.payload, resolved);
            // Read-only receiver: hoist the three words behind the payload
            // too. Loading them off an unresolved (0) payload would fault, so
            // they are read on the resolved path and zeroed on the reject
            // path — and `data == 0` is what the accesses test.
            if let Some(view) = cache.view {
                let lay = &helpers.array_layout;
                let ok = b.ins().icmp_imm(IntCC::NotEqual, resolved, 0);
                let load_blk = b.create_block();
                let skip = b.create_block();
                let merge = b.create_block();
                for _ in 0..3 {
                    b.append_block_param(merge, types::I64);
                }
                b.ins().brif(ok, load_blk, &[], skip, &[]);

                b.switch_to_block(load_blk);
                let data = b.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    resolved,
                    (16 + lay.elems_ptr_off) as i32,
                );
                let len = b.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    resolved,
                    (16 + lay.elems_len_off) as i32,
                );
                let disc = emit::array_disc(b, resolved, lay);
                b.ins().jump(merge, &[data.into(), len.into(), disc.into()]);

                b.switch_to_block(skip);
                let z0 = b.ins().iconst(types::I64, 0);
                b.ins().jump(merge, &[z0.into(), z0.into(), z0.into()]);

                b.switch_to_block(merge);
                for (i, v) in view.iter().enumerate() {
                    let p = b.block_params(merge)[i];
                    b.def_var(*v, p);
                }
            }
        }
    }
}
