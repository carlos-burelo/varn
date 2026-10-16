use cranelift_codegen::ir::{
    types, ExternalName, GlobalValueData, InstBuilder, UserExternalName, Value,
};
use cranelift_frontend::FunctionBuilder;
use rustc_hash::FxHashMap;
use std::sync::OnceLock;

use crate::JitHelpers;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AbsSym {
    Helper(u32),
    Literal(u32),
    Shape(u32),
    Native(u32),
    Epoch,
}

const NS_HELPER: u32 = 1;
const NS_LITERAL: u32 = 2;
const NS_SHAPE: u32 = 3;
const NS_NATIVE: u32 = 4;
const NS_EPOCH: u32 = 5;

impl AbsSym {
    fn name(self) -> UserExternalName {
        let (namespace, index) = match self {
            AbsSym::Helper(i) => (NS_HELPER, i),
            AbsSym::Literal(i) => (NS_LITERAL, i),
            AbsSym::Shape(i) => (NS_SHAPE, i),
            AbsSym::Native(i) => (NS_NATIVE, i),
            AbsSym::Epoch => (NS_EPOCH, 0),
        };
        UserExternalName::new(namespace, index)
    }

    pub(crate) fn from_name(name: &UserExternalName) -> Option<AbsSym> {
        Some(match name.namespace {
            NS_HELPER => AbsSym::Helper(name.index),
            NS_LITERAL => AbsSym::Literal(name.index),
            NS_SHAPE => AbsSym::Shape(name.index),
            NS_NATIVE => AbsSym::Native(name.index),
            NS_EPOCH => AbsSym::Epoch,
            _ => return None,
        })
    }
}

pub(in crate::clif) fn abs_value(b: &mut FunctionBuilder, sym: AbsSym) -> Value {
    let name = b.func.declare_imported_user_function(sym.name());
    let gv = b.create_global_value(GlobalValueData::Symbol {
        name: ExternalName::user(name),
        offset: 0.into(),
        colocated: false,
        tls: false,
    });
    b.ins().symbol_value(types::I64, gv)
}

static HELPER_ORDINALS: OnceLock<FxHashMap<usize, u32>> = OnceLock::new();

pub(crate) fn bind_helpers(helpers: &JitHelpers) {
    HELPER_ORDINALS.get_or_init(|| {
        let mut ordinals = FxHashMap::default();
        for (i, &addr) in helpers.call_targets.iter().enumerate() {
            ordinals.entry(addr).or_insert(i as u32);
        }
        ordinals
    });
}

thread_local! {
    static PINNED_ADDRESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn reset_pinned_address() {
    PINNED_ADDRESS.with(|c| c.set(false));
}

pub(crate) fn pinned_address() -> bool {
    PINNED_ADDRESS.with(|c| c.get())
}

pub(in crate::clif) fn helper_value(b: &mut FunctionBuilder, helper: usize) -> Value {
    match HELPER_ORDINALS.get().and_then(|m| m.get(&helper)) {
        Some(&ordinal) => abs_value(b, AbsSym::Helper(ordinal)),
        None => {
            if helper != 0 {
                PINNED_ADDRESS.with(|c| c.set(true));
            }
            b.ins().iconst(types::I64, helper as i64)
        }
    }
}

pub struct Resolver<'a> {
    pub helpers: &'a JitHelpers,
    pub proto: &'a varn_types::FunctionProto,
    pub constants: &'a [varn_types::VmValue],
    pub natives: &'a [u64],
    pub epoch: u64,
}

impl Resolver<'_> {
    pub(crate) fn resolve(&self, sym: AbsSym) -> Result<u64, String> {
        Ok(match sym {
            AbsSym::Helper(i) => *self
                .helpers
                .call_targets
                .get(i as usize)
                .ok_or_else(|| format!("reloc: helper #{i} out of range"))?
                as u64,
            AbsSym::Literal(i) => self
                .constants
                .get(i as usize)
                .ok_or_else(|| format!("reloc: literal #{i} out of range"))?
                .raw_payload(),
            AbsSym::Shape(i) => {
                let shape = self
                    .proto
                    .resolved_shape(i as usize)
                    .ok_or_else(|| format!("reloc: shape #{i} unresolved"))?;
                std::rc::Rc::as_ptr(&shape) as usize as u64
            }
            AbsSym::Native(i) => {
                let op = *self
                    .natives
                    .get(i as usize)
                    .ok_or_else(|| format!("reloc: native #{i} out of range"))?;
                (self.helpers.resolve_native_op)(op).func_ptr as u64
            }
            AbsSym::Epoch => self.epoch,
        })
    }
}
