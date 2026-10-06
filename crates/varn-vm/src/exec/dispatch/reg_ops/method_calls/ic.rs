


use crate::closure::VmClosure;
use crate::heap::Heap;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use varn_types::chunk::{CacheEntry, ICKind};
use varn_types::value::ClassObj;



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


pub(super) enum IcHit {
    Native(varn_types::NativeFn),
    
    Vm(Rc<VmClosure>, Rc<ClassObj>),
}




pub(super) fn probe(
    heap: &Heap,
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
            if let Some((f, _)) = method.and_then(|m| heap.native_of(*m)) {
                return Some(IcHit::Native(f));
            }
        } else if entry.is_class == ICKind::VM_VTABLE_METHOD {
            if let Some(nc) = method.and_then(|m| heap.closure_of(*m)) {
                if !nc.proto.is_generator && !nc.proto.is_async && arg_count <= nc.proto.arity {
                    return Some(IcHit::Vm(nc.clone(), cls.clone()));
                }
            }
        }
    }
    None
}








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
            class: Some(cls.clone()),
        };
        closure.ic_cache.borrow_mut()[site.cs].find_or_insert(entry);
        closure.feedback.borrow_mut().observe(site.cs, cls.id);
    }
}
