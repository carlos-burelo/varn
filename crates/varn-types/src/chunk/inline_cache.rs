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
}

#[derive(Clone, Copy, Default, Debug, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct CacheEntry {
    pub id: u32,
    pub slot: u16,
    pub is_class: u8,
    pub vtable_ver: u8,
}

impl CacheEntry {
    #[inline(always)]
    pub fn matches(&self, other: &CacheEntry) -> bool {
        self.id == other.id && self.is_class == other.is_class
    }
}

#[derive(Clone, Debug)]
pub struct PolyICSlot {
    pub entries: [CacheEntry; 8],

    pub next: u8,

    last_hit: u8,
}

impl Default for PolyICSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl PolyICSlot {
    pub fn new() -> Self {
        Self {
            entries: [CacheEntry::default(); 8],
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
