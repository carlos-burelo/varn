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

use super::emit::{
    self, call_helper, emit_array_payload, emit_object_data_base, ObjRegionCache, RegionCache,
    StrRegionCache,
};
use super::kinds::K;
use crate::JitHelpers;

/// Resolve every planned receiver of the region(s) whose header is `ip`.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_region_caches(
    b: &mut FunctionBuilder,
    helpers: &JitHelpers,
    cc: cranelift_codegen::isa::CallConv,
    exec_ctx: cranelift_codegen::ir::Value,
    vars: &[cranelift_frontend::Variable],
    cache_vars: &HashMap<(usize, usize), RegionCache>,
    str_caches: &HashMap<(usize, usize), StrRegionCache>,
    obj_caches: &HashMap<(usize, usize), ObjRegionCache>,
    regions: &[emit::Region],
    state: &[K],
    ip: usize,
) {
    for region in regions.iter().filter(|reg| reg.header == ip) {
        let h = &region.header;
        emit_str_caches(b, helpers, cc, exec_ctx, vars, str_caches, region, state);
        emit_obj_caches(b, helpers, exec_ctx, vars, obj_caches, region, state);
        for &r in &region.arrays {
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

                // Repr validation: when all ArrayGetIndex sites in the
                // loop agree on a single non-Boxed repr, validate the
                // disc here once. If it doesn't match, zero `data` so
                // every access in the body takes the slow path — the
                // body can then skip both repr branches entirely.
                if let Some(expected) = cache.repr_validated_disc {
                    let disc_ok = b.ins().icmp_imm(IntCC::Equal, disc, expected);
                    let repr_ok = b.create_block();
                    let repr_bad = b.create_block();
                    b.ins().brif(disc_ok, repr_ok, &[], repr_bad, &[]);

                    // disc matches expected — now check bounds hoisting.
                    b.switch_to_block(repr_ok);

                    // Check if this array has a bounds-hoistable access pattern.
                    let bh = region.bounds_hoistable.iter().find(|bh| bh.array_reg == r);
                    if let (Some(bh), Some(bound_reg)) = (bh, region.induction_bound) {
                        // Emit: max_index < len?
                        // For base+offset: (base + bound) <= len  (with overflow check)
                        // For direct:       bound <= len
                        let bound = b.use_var(vars[bound_reg]);
                        let bounds_ok = b.create_block();
                        let bounds_bad = b.create_block();

                        if let Some(base_r) = bh.base_reg {
                            let base = b.use_var(vars[base_r]);
                            // Checked add: base + bound. If it overflows, bounds fail.
                            let (sum, ovf) = b.ins().uadd_overflow(base, bound);
                            let no_ovf = b.create_block();
                            b.ins().brif(ovf, bounds_bad, &[], no_ovf, &[]);
                            b.switch_to_block(no_ovf);
                            // sum <= len  ⟺  !(sum > len)  ⟺  sum unsigned-le len
                            let ok = b.ins().icmp(IntCC::UnsignedLessThanOrEqual, sum, len);
                            b.ins().brif(ok, bounds_ok, &[], bounds_bad, &[]);
                        } else {
                            // Direct case: bound <= len
                            let ok = b.ins().icmp(IntCC::UnsignedLessThanOrEqual, bound, len);
                            b.ins().brif(ok, bounds_ok, &[], bounds_bad, &[]);
                        }

                        // Bounds OK: proceed with real data.
                        // bounds_guaranteed was already set statically in vars.rs
                        // because the preheader zeros data on failure.
                        b.switch_to_block(bounds_ok);
                        b.ins().jump(merge, &[data.into(), len.into(), disc.into()]);

                        // Bounds bad: zero data to trigger slow path.
                        b.switch_to_block(bounds_bad);
                        let z_b = b.ins().iconst(types::I64, 0);
                        b.ins().jump(merge, &[z_b.into(), z_b.into(), disc.into()]);
                    } else {
                        // No bounds hoisting for this array — proceed with real data.
                        b.ins().jump(merge, &[data.into(), len.into(), disc.into()]);
                    }

                    // disc doesn't match: set data=0 and len=0 to trigger slow path.
                    b.switch_to_block(repr_bad);
                    let z1 = b.ins().iconst(types::I64, 0);
                    b.ins().jump(merge, &[z1.into(), z1.into(), disc.into()]);
                } else {
                    b.ins().jump(merge, &[data.into(), len.into(), disc.into()]);
                }

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

/// Resolve each string receiver of `region` to a byte pointer and a length.
///
/// Two helper calls per receiver, once per loop entry — the alternative is
/// re-deriving both on every character. The helper answers `0` for anything it
/// cannot serve as flat bytes (SSO, non-ASCII, not a string), and `0` is the
/// same "unresolved" sentinel the array caches use, so the access sites need
/// exactly one test to choose between the inline load and the general helper.
///
/// The length is read on the resolved path only. Reading it for a rejected
/// receiver would be harmless here — it is a second call, not a load — but
/// keeping the two in lockstep is what lets an access treat `bytes != 0` as
/// proof that `len` describes the same string.
fn emit_str_caches(
    b: &mut FunctionBuilder,
    helpers: &JitHelpers,
    cc: cranelift_codegen::isa::CallConv,
    exec_ctx: cranelift_codegen::ir::Value,
    vars: &[cranelift_frontend::Variable],
    str_caches: &HashMap<(usize, usize), StrRegionCache>,
    region: &emit::Region,
    state: &[K],
) {
    for &r in &region.strings {
        let Some(cache) = str_caches.get(&(region.header, r)) else {
            continue;
        };
        if state[r] != K::Boxed {
            continue;
        }
        let recv = emit::box_or_pass(b, vars, state, r);
        let (recv_tag, recv_payload) = b.ins().isplit(recv);
        let bytes = call_helper(
            b,
            cc,
            helpers.str_ascii_bytes,
            &[exec_ctx, recv_tag, recv_payload],
        );

        let resolved = b.create_block();
        let rejected = b.create_block();
        let done = b.create_block();
        b.append_block_param(done, types::I64);
        b.ins().brif(bytes, resolved, &[], rejected, &[]);

        b.switch_to_block(resolved);
        let len = call_helper(
            b,
            cc,
            helpers.str_ascii_len,
            &[exec_ctx, recv_tag, recv_payload],
        );
        b.ins().jump(done, &[len.into()]);

        b.switch_to_block(rejected);
        let zero = b.ins().iconst(types::I64, 0);
        b.ins().jump(done, &[zero.into()]);

        b.switch_to_block(done);
        b.def_var(cache.bytes, bytes);
        let len = b.block_params(done)[0];
        b.def_var(cache.len, len);
    }
}

/// Resolve each object receiver of `region` to its inline field base address.
fn emit_obj_caches(
    b: &mut FunctionBuilder,
    helpers: &JitHelpers,
    exec_ctx: cranelift_codegen::ir::Value,
    vars: &[cranelift_frontend::Variable],
    obj_caches: &HashMap<(usize, usize), ObjRegionCache>,
    region: &emit::Region,
    state: &[K],
) {
    for &r in &region.objects {
        let Some(cache) = obj_caches.get(&(region.header, r)) else {
            continue;
        };
        if state[r] != K::Boxed {
            continue;
        }
        let obj = b.use_var(vars[r]);
        let invalid = b.create_block();
        let done = b.create_block();
        b.append_block_param(done, types::I64);

        let data_base = emit_object_data_base(
            b,
            exec_ctx,
            obj,
            &helpers.object_layout,
            &helpers.array_layout,
            helpers.heap_field_offset,
            invalid,
        );
        b.ins().jump(done, &[data_base.into()]);

        b.switch_to_block(invalid);
        let z = b.ins().iconst(types::I64, 0);
        b.ins().jump(done, &[z.into()]);

        b.switch_to_block(done);
        let resolved = b.block_params(done)[0];
        b.def_var(cache.data_base, resolved);
    }
}
