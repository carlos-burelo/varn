//! Inline-cache slots and the per-site feedback the JIT reads: shape ids,
//! polymorphic slots, and call-site profiles.

pub const INVALID_CACHE_SHAPE: u32 = 0;

/// Classification flags for Inline Cache (IC) slot entries (`CacheEntry.is_class`).
pub struct ICKind;
impl ICKind {
    /// Object / Record field access by shape ID
    pub const SHAPE_PROP: u8 = 1;
    /// Class instance method on vtable (GetProperty)
    pub const CLASS_METHOD: u8 = 2;
    /// Class getter accessor on vtable (GetProperty)
    pub const CLASS_GETTER: u8 = 3;
    /// Class setter accessor on vtable (SetProperty)
    pub const CLASS_SETTER: u8 = 4;
    /// Object shape transition (SetProperty)
    pub const SHAPE_TRANSITION: u8 = 5;
    /// Native function on class / intrinsic vtable (CallMethod)
    pub const NATIVE_VTABLE_METHOD: u8 = 6;
    /// VM closure on class / intrinsic vtable (CallMethod)
    pub const VM_VTABLE_METHOD: u8 = 7;
    /// Array `.length` property access
    pub const ARRAY_LENGTH: u8 = 8;
    /// String `.length` property access
    pub const STR_LENGTH: u8 = 9;
    /// Class instance field access by slot (GetProperty / SetProperty)
    pub const INSTANCE_FIELD: u8 = 10;
}

#[derive(Clone, Default, Debug, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct CacheEntry {
    pub id: u32,
    pub slot: u16,
    pub is_class: u8,
    pub vtable_ver: u8,
    /// Owning pointer to the class this entry was recorded against, present
    /// only for vtable kinds (`CLASS_METHOD`, `CLASS_GETTER`,
    /// `CLASS_SETTER`, `NATIVE_VTABLE_METHOD`, `VM_VTABLE_METHOD`). Lets
    /// generated code go from entry to vtable without touching the class
    /// registry: the `Rc` keeps the class alive exactly as long as the site
    /// that cached it, so the pointer can never dangle. Shape/instance-field
    /// and length kinds carry `None` — their fast paths key off the shape id
    /// or class id alone and never read a vtable.
    ///
    /// Appended last on purpose: the JIT bakes the offsets of the first four
    /// fields (`id`@0, `slot`@4, `is_class`@6, `vtable_ver`@7) and must not
    /// shift when this grows.
    #[serde(skip)]
    pub class: Option<std::rc::Rc<crate::value::ClassObj>>,
}

impl CacheEntry {
    #[inline(always)]
    pub fn matches(&self, other: &CacheEntry) -> bool {
        self.id == other.id && self.is_class == other.is_class
    }
}

/// `#[repr(C)]` so the JIT can index the entries array directly: `entries` is
/// at offset 0, one slot is `POLY_IC_SLOT_SIZE` bytes.
#[derive(Clone, Debug)]
#[repr(C)]
pub struct PolyICSlot {
    pub entries: [CacheEntry; 8],

    pub next: u8,

    last_hit: u8,
}

/// `size_of::<PolyICSlot>()` — the stride the JIT uses to reach slot `cs`.
/// 8 entries of 16 bytes plus the two trailing `u8`s, padded to alignment 8.
pub const POLY_IC_SLOT_SIZE: usize = 136;

impl Default for PolyICSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl PolyICSlot {
    pub fn new() -> Self {
        const _: () = assert!(std::mem::size_of::<PolyICSlot>() == POLY_IC_SLOT_SIZE);
        // JIT-baked entry offsets (see `CacheEntry::class`): id@0, slot@4,
        // is_class@6, vtable_ver@7, class@8. If a field moves, the inline
        // guards in `varn-jit` read garbage — fail here, not in production.
        const _: () = assert!(std::mem::offset_of!(CacheEntry, id) == 0);
        const _: () = assert!(std::mem::offset_of!(CacheEntry, slot) == 4);
        const _: () = assert!(std::mem::offset_of!(CacheEntry, is_class) == 6);
        const _: () = assert!(std::mem::offset_of!(CacheEntry, vtable_ver) == 7);
        const _: () = assert!(std::mem::offset_of!(CacheEntry, class) == 8);
        Self {
            entries: std::array::from_fn(|_| CacheEntry::default()),
            next: 0,
            last_hit: 0,
        }
    }

    pub fn find_or_insert(&mut self, entry: CacheEntry) {
        for (i, e) in self.entries.iter_mut().enumerate() {
            if e.matches(&entry) {
                *e = entry;
                self.last_hit = i as u8;

                self.next = (self.last_hit + 4) & 0x7;
                return;
            }
        }

        self.entries[self.next as usize] = entry;
        self.next = (self.next + 1) & 0x7;
    }
}

#[derive(Clone, Debug, Default)]
pub struct SiteProfile {
    pub ids: [u32; 4],
    pub count: u32,
    pub megamorphic: bool,
}

impl SiteProfile {
    #[inline(always)]
    pub fn observe(&mut self, id: u32) {
        if self.megamorphic || id == 0 {
            return;
        }
        self.count = self.count.saturating_add(1);
        for slot in &mut self.ids {
            if *slot == id {
                return;
            }
            if *slot == 0 {
                *slot = id;
                return;
            }
        }
        self.megamorphic = true;
    }
}

#[derive(Clone, Debug, Default)]
pub struct FeedbackVector {
    pub sites: Vec<SiteProfile>,
}

impl FeedbackVector {
    pub fn new(site_count: usize) -> Self {
        Self {
            sites: vec![SiteProfile::default(); site_count],
        }
    }

    #[inline(always)]
    pub fn observe(&mut self, site_idx: usize, id: u32) {
        if let Some(site) = self.sites.get_mut(site_idx) {
            site.observe(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contrato del consumidor JIT: offsets horneados + stride + puntero de
    /// clase en entradas vtable. Si esto falla, los guards inline leen basura.
    #[test]
    fn jit_entry_contract() {
        assert_eq!(std::mem::offset_of!(CacheEntry, id), 0);
        assert_eq!(std::mem::offset_of!(CacheEntry, slot), 4);
        assert_eq!(std::mem::offset_of!(CacheEntry, is_class), 6);
        assert_eq!(std::mem::offset_of!(CacheEntry, vtable_ver), 7);
        assert_eq!(std::mem::offset_of!(CacheEntry, class), 8);
        assert_eq!(std::mem::size_of::<CacheEntry>(), 16);
        assert_eq!(std::mem::size_of::<PolyICSlot>(), POLY_IC_SLOT_SIZE);

        // Una entrada vtable retiene su clase; una de shape no carga ninguna.
        let cls = std::rc::Rc::new(crate::value::ClassObj::new("Probe"));
        let mut slot = PolyICSlot::new();
        slot.find_or_insert(CacheEntry {
            id: cls.id,
            slot: 3,
            is_class: ICKind::VM_VTABLE_METHOD,
            vtable_ver: 1,
            class: Some(cls.clone()),
        });
        slot.find_or_insert(CacheEntry {
            id: 99,
            slot: 0,
            is_class: ICKind::SHAPE_PROP,
            vtable_ver: 0,
            class: None,
        });
        let hit = slot
            .entries
            .iter()
            .find(|e| e.id == cls.id && e.is_class == ICKind::VM_VTABLE_METHOD)
            .expect("vtable entry recorded");
        assert_eq!(hit.slot, 3);
        assert!(std::rc::Rc::ptr_eq(hit.class.as_ref().unwrap(), &cls));
        let shape = slot
            .entries
            .iter()
            .find(|e| e.is_class == ICKind::SHAPE_PROP)
            .expect("shape entry recorded");
        assert!(shape.class.is_none());
    }
}
