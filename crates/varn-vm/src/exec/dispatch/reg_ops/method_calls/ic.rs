//! A method call site's inline cache: the probe that answers a call from it
//! and the one place that records a resolution into it.

use crate::closure::{VmClosure, VmClosurePayload};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use varn_types::chunk::{CacheEntry, ICKind};
use varn_types::value::ClassObj;
use varn_types::Value;

/// Cache slot `cs` of the calling closure, usable when the closure has that
/// slot and the site has not gone megamorphic. `usize::MAX` names no slot.
#[derive(Clone, Copy)]
pub(super) struct IcSite {
    cs: usize,
    usable: bool,
}

impl IcSite {
    pub(super) fn of(closure: &VmClosure, cs: usize) -> Self {
        let megamorphic = closure
            .feedback
            .borrow()
            .sites
            .get(cs)
            .is_some_and(|s| s.megamorphic);
        IcSite {
            cs,
            usable: cs < closure.ic_cache_len() && !megamorphic,
        }
    }

    pub(super) fn usable(self) -> bool {
        self.usable
    }
}

/// What a cache entry resolved the call to.
pub(super) enum IcHit {
    Native(varn_types::NativeFn),
    /// A VM method, with the receiver's class as the frame's current class.
    Vm(Rc<VmClosure>, Rc<ClassObj>),
}

/// The entry of `site` that matches receiver class `cls` at its current
/// vtable version, if any. A VM method is only answered from the cache when
/// it takes a plain frame (not a generator, not async) and `arg_count` fits.
pub(super) fn probe(
    closure: &VmClosure,
    site: IcSite,
    cls: &Rc<ClassObj>,
    arg_count: usize,
) -> Option<IcHit> {
    if !site.usable {
        return None;
    }
    let ic = unsafe { &*closure.ic_cache.as_ptr() };
    let cls_ver = (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8;
    for entry in &ic[site.cs].entries {
        if entry.id == 0 || entry.id != cls.id || entry.vtable_ver != cls_ver {
            continue;
        }
        let vtable = unsafe { &*cls.vtable.as_ptr() };
        let method = vtable.get(entry.slot as usize);
        if entry.is_class == ICKind::NATIVE_VTABLE_METHOD {
            if let Some(Value::NativeFn(b)) = method {
                return Some(IcHit::Native(b.0));
            }
        } else if entry.is_class == ICKind::VM_VTABLE_METHOD {
            if let Some(Value::VmValue(payload)) = method {
                if let Some(nc) = VmClosurePayload::downcast_from(&**payload) {
                    if !nc.proto.is_generator && !nc.proto.is_async && arg_count <= nc.proto.arity {
                        return Some(IcHit::Vm(nc.clone(), cls.clone()));
                    }
                }
            }
        }
    }
    None
}

/// Record in `site` that receiver class `cls` resolves `name` to its vtable
/// method of kind `kind` (an [`ICKind`]).
///
/// Caching only ever RECORDS what the class already has: a name missing
/// from the class's `method_map` (a property, a prototype method) is not
/// cached. Installing it would bump `vtable_version` on the first call,
/// invalidating every other entry cached against that class.
pub(super) fn record(
    closure: &VmClosure,
    site: IcSite,
    cls: Option<&Rc<ClassObj>>,
    name: &str,
    kind: u8,
) {
    if !site.usable {
        return;
    }
    let Some(cls) = cls else { return };
    if let Some(&slot) = cls.method_map.borrow().get(name) {
        let entry = CacheEntry {
            id: cls.id,
            slot: slot as u16,
            is_class: kind,
            vtable_ver: (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8,
        };
        closure.ic_cache.borrow_mut()[site.cs].find_or_insert(entry);
        closure.feedback.borrow_mut().observe(site.cs, cls.id);
    }
}
