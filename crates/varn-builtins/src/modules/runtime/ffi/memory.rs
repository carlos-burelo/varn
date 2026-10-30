use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use varn_types::NativeError;

const ALIGN: usize = 16;
const FOREIGN_CSTRING_SCAN: usize = 65536;

static OWNED: Mutex<BTreeMap<usize, usize>> = Mutex::new(BTreeMap::new());

fn owned() -> MutexGuard<'static, BTreeMap<usize, usize>> {
    OWNED.lock().unwrap_or_else(PoisonError::into_inner)
}

fn fault(who: &str, message: impl std::fmt::Display) -> NativeError {
    NativeError::range_error(format!("ffi.{who}: {message}"))
}

fn layout(who: &str, size: usize) -> Result<Layout, NativeError> {
    Layout::from_size_align(size, ALIGN).map_err(|e| fault(who, format!("invalid layout: {e}")))
}

fn containing(map: &BTreeMap<usize, usize>, addr: usize) -> Option<(usize, usize)> {
    map.range(..=addr)
        .next_back()
        .filter(|(base, size)| addr < **base + **size)
        .map(|(base, size)| (*base, *size))
}

pub fn allocate(who: &str, size: i64) -> Result<usize, NativeError> {
    let size = usize::try_from(size)
        .ok()
        .filter(|size| *size > 0)
        .ok_or_else(|| fault(who, "size must be greater than 0"))?;
    let layout = layout(who, size)?;
    let raw = unsafe { alloc_zeroed(layout) };
    if raw.is_null() {
        return Err(NativeError::std(
            varn_core::StdErrorClass::Std,
            format!("ffi.{who}: out of memory"),
        ));
    }
    owned().insert(raw as usize, size);
    Ok(raw as usize)
}

pub fn allocate_cstring(who: &str, text: &str) -> Result<usize, NativeError> {
    let bytes = text.as_bytes();
    let addr = allocate(who, bytes.len() as i64 + 1)?;
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len()) };
    Ok(addr)
}

pub fn release(who: &str, addr: usize) -> Result<(), NativeError> {
    if addr == 0 {
        return Ok(());
    }
    let size = owned()
        .remove(&addr)
        .ok_or_else(|| fault(who, "pointer was not returned by alloc, or was already freed"))?;
    unsafe { dealloc(addr as *mut u8, layout(who, size)?) };
    Ok(())
}

fn at(who: &str, addr: i64, offset: i64) -> Result<usize, NativeError> {
    if addr == 0 {
        return Err(fault(who, "null pointer dereference"));
    }
    addr.checked_add(offset)
        .and_then(|effective| usize::try_from(effective).ok())
        .ok_or_else(|| fault(who, "pointer offset out of range"))
}

pub fn span(who: &str, addr: i64, offset: i64, width: usize) -> Result<usize, NativeError> {
    let effective = at(who, addr, offset)?;
    let map = owned();
    let Some((base, size)) = containing(&map, addr as usize) else {
        return Ok(effective);
    };
    let within = effective >= base
        && effective
            .checked_add(width)
            .is_some_and(|end| end <= base + size);
    if within {
        Ok(effective)
    } else {
        Err(fault(
            who,
            format!("access of {width} bytes at offset {offset} is outside the {size}-byte allocation"),
        ))
    }
}

pub fn cstring_bounds(who: &str, addr: i64, offset: i64) -> Result<(usize, usize), NativeError> {
    let start = at(who, addr, offset)?;
    let map = owned();
    let limit = match containing(&map, addr as usize) {
        Some((base, size)) if start >= base && start < base + size => base + size - start,
        Some(_) => return Err(fault(who, "offset is outside the allocation")),
        None => FOREIGN_CSTRING_SCAN,
    };
    Ok((start, limit))
}
