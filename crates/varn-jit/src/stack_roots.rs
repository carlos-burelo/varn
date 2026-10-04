use std::collections::BTreeMap;
use std::sync::RwLock;

use varn_types::VmValue;

pub(crate) struct SafepointMap {
    pub return_offset: u32,
    pub slots: Box<[u32]>,
}

struct CodeRange {
    end: usize,
    sites: Box<[SafepointMap]>,
}

static CODE: RwLock<BTreeMap<usize, CodeRange>> = RwLock::new(BTreeMap::new());

pub(crate) fn register(start: usize, len: usize, mut sites: Vec<SafepointMap>) {
    sites.sort_by_key(|s| s.return_offset);
    CODE.write().expect("stack roots registry").insert(
        start,
        CodeRange {
            end: start + len,
            sites: sites.into_boxed_slice(),
        },
    );
}

pub(crate) fn unregister(start: usize) {
    CODE.write().expect("stack roots registry").remove(&start);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct JitExit {
    pub sp: usize,
    pub fp: usize,
}

/// Visits every GC slot of the compiled frames that `exit` opens, youngest
/// first, stopping at the first return address that is not compiled code.
///
/// # Safety
///
/// `exit` must be the record a compiled frame left while it is suspended in
/// a call: its stack, and every compiled caller above it, still live.
pub unsafe fn for_each_slot(exit: JitExit, mut visit: impl FnMut(*mut VmValue)) {
    if exit.sp == 0 {
        return;
    }
    let code = CODE.read().expect("stack roots registry");
    let mut sp = exit.sp;
    let mut fp = exit.fp;
    let mut ra = *((sp - 8) as *const usize);
    loop {
        let Some((start, range)) = code.range(..=ra).next_back() else {
            return;
        };
        if ra >= range.end {
            return;
        }
        let offset = (ra - start) as u32;
        if let Ok(i) = range
            .sites
            .binary_search_by_key(&offset, |s| s.return_offset)
        {
            for &slot in range.sites[i].slots.iter() {
                visit((sp + slot as usize) as *mut VmValue);
            }
        }
        ra = *((fp + 8) as *const usize);
        sp = fp + 16;
        fp = *(fp as *const usize);
    }
}
