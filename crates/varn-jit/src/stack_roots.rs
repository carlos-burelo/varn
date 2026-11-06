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
    locations: Box<[(u32, u32)]>,
    owner: usize,
}

static CODE: RwLock<BTreeMap<usize, CodeRange>> = RwLock::new(BTreeMap::new());

#[cfg(feature = "clif")]
pub(crate) fn register(
    start: usize,
    len: usize,
    mut sites: Vec<SafepointMap>,
    mut locations: Vec<(u32, u32)>,
    owner: usize,
) {
    sites.sort_by_key(|s| s.return_offset);
    locations.sort_by_key(|(offset, _)| *offset);
    CODE.write().expect("stack roots registry").insert(
        start,
        CodeRange {
            end: start + len,
            sites: sites.into_boxed_slice(),
            locations: locations.into_boxed_slice(),
            owner,
        },
    );
}

pub struct CodeLocation {
    pub owner: usize,
    pub ip: u32,
}

pub fn locate(address: usize) -> Option<CodeLocation> {
    let code = CODE.try_read().ok()?;
    let (start, range) = code.range(..=address).next_back()?;
    if address >= range.end {
        return None;
    }
    let offset = (address - start) as u32;
    let at = range.locations.partition_point(|(from, _)| *from <= offset);
    let (_, ip) = range.locations.get(at.checked_sub(1)?)?;
    Some(CodeLocation {
        owner: range.owner,
        ip: *ip,
    })
}

#[cfg(feature = "clif")]
pub(crate) fn unregister(start: usize) {
    CODE.write().expect("stack roots registry").remove(&start);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct JitExit {
    pub sp: usize,
    pub fp: usize,
}

pub unsafe fn for_each_slot(exit: JitExit, mut visit: impl FnMut(*mut VmValue)) {
    if exit.sp == 0 {
        return;
    }
    let code = CODE.read().expect("stack roots registry");
    let mut sp = exit.sp;
    let mut fp = exit.fp;
    let mut ra = unsafe { *((sp - 8) as *const usize) };
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
        ra = unsafe { *((fp + 8) as *const usize) };
        sp = fp + 16;
        fp = unsafe { *(fp as *const usize) };
    }
}
