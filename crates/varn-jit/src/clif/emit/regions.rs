//! What a loop region hoisted to its preheader: array, string and object
//! caches, induction facts.

use super::*;

/// What a loop region knows about one array receiver, resolved once in the
/// region's preheader.
///
/// `payload` alone (the pointer to the array's `ArrayRepr`) is what every
/// receiver gets: it skips the tag/generation/slot walk on each access.
///
/// `view` is the stronger form, and only a receiver the region never WRITES
/// can have it, inside a region that never allocates. Under those two facts
/// the element pointer, the length and the repr discriminant are all
/// loop-invariant, so an access is a bounds compare, a repr compare and one
/// load — no resolve, and no reload of the three words behind it. A written
/// receiver keeps `view: None`: a store can grow the element Vec (moving the
/// data pointer and the length) or migrate the repr.
#[derive(Clone, Copy)]
pub(crate) struct RegionCache {
    /// Payload pointer; `0` means the preheader's guard chain rejected it.
    pub payload: Variable,
    /// `[data, len, disc]`, sharing the same `0`-means-unresolved sentinel on
    /// `data` (a live `Vec`'s pointer is never null, empty or not).
    pub view: Option<[Variable; 3]>,
    /// When `Some(disc)`, the preheader validated that the array's repr
    /// discriminant equals `disc` before entering the loop. If the repr
    /// does NOT match, the preheader sets `data = 0`, which routes every
    /// access to the generic helper. Therefore, inside the loop body,
    /// `data != 0` already guarantees `disc == expected`, and access sites
    /// can skip both repr branches (the raw arm and the boxed fallback).
    ///
    /// Sound because `view` is only set in alloc-free regions, and no
    /// operation in an alloc-free region can change an array's repr
    /// (specialization happens on push, migration on mismatched write —
    /// both allocate or call, which the allowlist excludes).
    pub repr_validated_disc: Option<i64>,
    /// When true, the preheader verified that ALL induction-variable-based
    /// indices into this array are within bounds (max_index < len).
    /// The per-iteration bounds check can be skipped entirely — `data != 0`
    /// already guarantees both repr match AND bounds safety.
    #[allow(dead_code)]
    pub bounds_guaranteed: bool,
}

/// What a loop region knows about one string receiver, resolved once in the
/// region's preheader: the address of its bytes and how many there are.
///
/// Only a receiver whose content is **directly byte-indexable** gets one —
/// heap-allocated and ASCII, so byte index equals character index. Anything
/// else (SSO, non-ASCII, not a string at all) leaves `bytes` at `0` and every
/// access in the body falls back to the helper, which handles the general
/// case. The preconditions are the same two that let an array receiver carry a
/// `view`: the region redefines nothing it caches, and it allocates nothing —
/// so no collection and no `Vec` growth can move what `bytes` points at, and
/// no `str_concat` can extend an `Ext` buffer under it.
#[derive(Clone, Copy)]
pub(crate) struct StrRegionCache {
    /// Pointer to the first byte; `0` means the preheader rejected the
    /// receiver, exactly like `RegionCache::payload`.
    pub bytes: Variable,
    /// Byte length, meaningful only when `bytes != 0`.
    pub len: Variable,
}

/// What a loop region knows about one object receiver, resolved once in the
/// region's preheader: the base pointer to its inline field values.
#[derive(Clone, Copy)]
pub(crate) struct ObjRegionCache {
    /// Base address of inline fields (`objdata + values_off`); `0` means the
    /// preheader rejected the receiver.
    pub data_base: Variable,
}

/// An array access inside a loop that can have its bounds check hoisted to the
/// preheader because the index is derived from the loop induction variable.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BoundsHoistable {
    /// Register of the array receiver.
    pub array_reg: usize,
    /// Register of the loop-invariant base, if the index pattern is `base + k` or `base + k * stride`.
    /// `None` if the index has no additive base.
    pub base_reg: Option<usize>,
    /// Register of the loop-invariant stride, if the index pattern is `k * stride` or `base + k * stride`.
    /// `None` if the index has unit stride (stride = 1).
    pub stride_reg: Option<usize>,
}

/// One loop region and everything hoistable out of it.
///
/// A struct rather than the 4-tuple this used to be: the fields are all
/// `usize`/`Vec<usize>`, so a transposed pair compiles and silently hoists the
/// wrong register — the same defect `NativeOpTarget` was introduced to close.
pub(crate) struct Region {
    /// First ip of the loop body.
    pub header: usize,
    /// ip of the `Loop` op that closes it; the region is `[header, back_edge)`.
    pub back_edge: usize,
    /// Array receivers the region never redefines.
    pub arrays: Vec<usize>,
    /// Of [`Self::arrays`], those the region never writes either.
    pub read_only: Vec<usize>,
    /// Every char-indexing intrinsic in the region that can be served from a
    /// hoisted byte view, as `(ip of the intrinsic, register its receiver was
    /// copied from)`.
    pub string_sites: Vec<(usize, usize)>,
    /// The distinct receivers of [`Self::string_sites`]: one cache each.
    pub strings: Vec<usize>,
    /// Object receivers of fixed-field accesses that the region never redefines.
    pub objects: Vec<usize>,
    /// Bytecode offsets of induction variable increments (e.g. AddImm / SubImm)
    /// proven to operate on bounded loop index variables.
    pub induction_increments: Vec<usize>,
    /// Register of the loop induction variable (left operand of the header comparison).
    #[allow(dead_code)]
    pub induction_var: Option<usize>,
    /// Register of the loop bound (right operand of the header comparison).
    pub induction_bound: Option<usize>,
    /// Array accesses whose bounds check can be hoisted to the preheader.
    pub bounds_hoistable: Vec<BoundsHoistable>,
    /// Bytecode offsets of AddInt operations that compute induction-based
    /// indices — proven unable to overflow when bounds are guaranteed.
    pub bounds_safe_arith: Vec<usize>,
}

impl Region {
    fn contains_ip(&self, ip: usize) -> bool {
        self.header <= ip && ip < self.back_edge
    }
}

/// Everything the loop regions hoisted, as one value.
///
/// The regions and the cache maps are only ever meaningful together — a
/// cache is looked up BY the region that owns it — so they travel together
/// rather than as parallel parameters that a call site could pair up
/// wrongly.
#[derive(Clone, Copy)]
pub(crate) struct LoopCaches<'a> {
    pub regions: &'a [Region],
    pub arrays: &'a HashMap<(usize, usize), RegionCache>,
    pub strings: &'a HashMap<(usize, usize), StrRegionCache>,
}

impl LoopCaches<'_> {
    /// Whether an AddImm / SubImm instruction at `ip` is an induction variable increment
    /// inside a bounded loop region, exempt from hardware overflow checks.
    pub(in crate::clif) fn is_induction_increment(&self, ip: usize) -> bool {
        self.regions
            .iter()
            .any(|reg| reg.induction_increments.contains(&ip))
    }

    /// Whether an AddInt at `ip` is an index computation for a bounds-guaranteed
    /// array access, proven unable to overflow because the preheader validated
    /// that the maximum index fits within the array length (which fits in i64).
    pub(in crate::clif) fn is_bounds_safe_arith(&self, ip: usize) -> bool {
        self.regions
            .iter()
            .any(|reg| reg.bounds_safe_arith.contains(&ip))
    }

    /// The array-payload cache `r` should use at `ip`.
    pub(in crate::clif) fn array(&self, ip: usize, r: usize) -> Option<RegionCache> {
        self.find(ip, r, |reg| &reg.arrays, self.arrays)
    }

    /// The object-data cache `r` should use at `ip`.
    ///
    /// Fase B: disabled. The hoisted base is `InstanceData::payload` for a
    /// class instance, whose fields are COMPACT (`TypeLayout::of_field`); the
    /// access sites then load at `slot*16`, wrong for instances. Until the
    /// cache records the receiver's shape (instance vs dynamic object), no
    /// object access is hoisted — `GetFixedField`/`SetFixedField` route
    /// instances to the compact-aware helper.
    pub(in crate::clif) fn object(&self, _ip: usize, _r: usize) -> Option<ObjRegionCache> {
        None
    }

    /// The hoisted byte view the char-indexing intrinsic AT `ip` reads from,
    /// if its region planned one. Keyed by the site rather than by a register
    /// the access site would have to re-derive.
    pub(in crate::clif) fn string_at(&self, ip: usize) -> Option<StrRegionCache> {
        self.regions
            .iter()
            .filter(|reg| reg.contains_ip(ip))
            .filter_map(|reg| {
                let (_, r) = reg.string_sites.iter().find(|(site, _)| *site == ip)?;
                Some((reg, *r))
            })
            .min_by_key(|(reg, _)| reg.back_edge - reg.header)
            .and_then(|(reg, r)| self.strings.get(&(reg.header, r)).copied())
    }

    /// The cache belonging to the INNERMOST region that both contains `ip` and
    /// hoisted `r`. One lookup rule for all kinds, so they cannot drift apart.
    fn find<C: Copy>(
        &self,
        ip: usize,
        r: usize,
        receivers: impl Fn(&Region) -> &[usize],
        cache_vars: &HashMap<(usize, usize), C>,
    ) -> Option<C> {
        self.regions
            .iter()
            .filter(|reg| reg.contains_ip(ip) && receivers(reg).contains(&r))
            .min_by_key(|reg| reg.back_edge - reg.header)
            .map(|reg| cache_vars[&(reg.header, r)])
    }
}
