#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SlotKind {
    Int,
    Float,
    Bool,

    Str,

    Ref,
    Dynamic,
}

impl Default for SlotKind {
    fn default() -> Self {
        SlotKind::Dynamic
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum SlotClass {
    Gpr,
    Fpr,
    Ref,
    Dyn,
}

impl SlotClass {
    #[inline(always)]
    pub fn of_kind(kind: SlotKind) -> Self {
        match kind {
            SlotKind::Int => SlotClass::Gpr,
            SlotKind::Float => SlotClass::Fpr,
            SlotKind::Ref => SlotClass::Ref,
            SlotKind::Dynamic | SlotKind::Bool | SlotKind::Str => SlotClass::Dyn,
        }
    }

    #[inline(always)]
    pub fn index(self) -> usize {
        match self {
            SlotClass::Gpr => 0,
            SlotClass::Fpr => 1,
            SlotClass::Ref => 2,
            SlotClass::Dyn => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RegisterMeta {
    pub kind: SlotKind,
}

#[derive(Debug)]
pub struct FrameLayout {
    pub slots: Vec<(SlotClass, u32)>,

    pub counts: [u32; 4],
}

impl FrameLayout {
    pub fn for_proto(proto: &crate::FunctionProto) -> Self {
        let n = proto.register_count as usize;
        let mut slots = Vec::with_capacity(n);
        let mut counts = [0u32; 4];
        for r in 0..n {
            let kind = proto
                .register_meta
                .get(r)
                .map(|m| m.kind)
                .unwrap_or(SlotKind::Dynamic);
            let class = SlotClass::of_kind(kind);
            let idx = counts[class.index()];
            counts[class.index()] += 1;
            slots.push((class, idx));
        }
        Self { slots, counts }
    }

    #[inline(always)]
    pub fn class_of(&self, reg: usize) -> SlotClass {
        self.slots
            .get(reg)
            .map(|(c, _)| *c)
            .unwrap_or(SlotClass::Dyn)
    }

    #[inline(always)]
    pub fn idx_of(&self, reg: usize) -> u32 {
        self.slots.get(reg).map(|(_, i)| *i).unwrap_or(0)
    }
}
