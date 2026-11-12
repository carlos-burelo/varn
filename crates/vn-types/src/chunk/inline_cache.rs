pub struct ICKind;
impl ICKind {
    pub const SHAPE_PROP: u8 = 1;

    pub const CLASS_METHOD: u8 = 2;

    pub const CLASS_GETTER: u8 = 3;

    pub const CLASS_SETTER: u8 = 4;

    pub const SHAPE_TRANSITION: u8 = 5;

    pub const NATIVE_VTABLE_METHOD: u8 = 6;

    pub const VM_VTABLE_METHOD: u8 = 7;

    pub const ARRAY_LENGTH: u8 = 8;

    pub const STR_LENGTH: u8 = 9;

    pub const INSTANCE_FIELD: u8 = 10;
}

#[derive(Clone, Default, Debug, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct CacheEntry {
    pub id: u32,
    pub slot: u16,
    pub is_class: u8,
    pub vtable_ver: u8,

    #[serde(skip)]
    pub class: Option<std::rc::Rc<crate::value::ClassObj>>,

    pub offset: u32,
    pub access: u8,
}

impl CacheEntry {
    #[inline(always)]
    pub fn matches(&self, other: &CacheEntry) -> bool {
        self.id == other.id && self.is_class == other.is_class
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
pub struct PolyICSlot {
    pub entries: [CacheEntry; 8],

    pub next: u8,

    last_hit: u8,
}

pub const POLY_IC_SLOT_SIZE: usize = 200;

impl Default for PolyICSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl PolyICSlot {
    pub fn new() -> Self {
        const _: () = assert!(std::mem::size_of::<PolyICSlot>() == POLY_IC_SLOT_SIZE);

        const _: () = assert!(std::mem::offset_of!(CacheEntry, id) == 0);
        const _: () = assert!(std::mem::offset_of!(CacheEntry, slot) == 4);
        const _: () = assert!(std::mem::offset_of!(CacheEntry, is_class) == 6);
        const _: () = assert!(std::mem::offset_of!(CacheEntry, vtable_ver) == 7);
        const _: () = assert!(std::mem::offset_of!(CacheEntry, class) == 8);
        const _: () = assert!(std::mem::offset_of!(CacheEntry, offset) == 16);
        const _: () = assert!(std::mem::offset_of!(CacheEntry, access) == 20);
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
