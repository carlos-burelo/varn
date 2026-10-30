use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

const ALIGN: usize = 16;
const FOREIGN_CSTRING_SCAN: usize = 65536;

static OWNED: Mutex<BTreeMap<usize, usize>> = Mutex::new(BTreeMap::new());

fn owned() -> MutexGuard<'static, BTreeMap<usize, usize>> {
    OWNED.lock().unwrap_or_else(PoisonError::into_inner)
}

fn layout(size: usize) -> Result<Layout, String> {
    Layout::from_size_align(size, ALIGN).map_err(|e| format!("invalid layout: {e}"))
}

fn containing(map: &BTreeMap<usize, usize>, addr: usize) -> Option<(usize, usize)> {
    map.range(..=addr)
        .next_back()
        .filter(|(base, size)| addr < **base + **size)
        .map(|(base, size)| (*base, *size))
}

pub fn allocate(size: i64) -> Result<usize, String> {
    let size = usize::try_from(size)
        .ok()
        .filter(|size| *size > 0)
        .ok_or_else(|| "size must be greater than 0".to_string())?;
    let layout = layout(size)?;
    let raw = unsafe { alloc_zeroed(layout) };
    if raw.is_null() {
        return Err("out of memory".to_string());
    }
    owned().insert(raw as usize, size);
    Ok(raw as usize)
}

pub fn allocate_cstring(text: &str) -> Result<usize, String> {
    let bytes = text.as_bytes();
    let addr = allocate(bytes.len() as i64 + 1)?;
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len()) };
    Ok(addr)
}

pub fn release(addr: usize) -> Result<(), String> {
    if addr == 0 {
        return Ok(());
    }
    let size = owned()
        .remove(&addr)
        .ok_or_else(|| "pointer was not returned by alloc, or was already freed".to_string())?;
    unsafe { dealloc(addr as *mut u8, layout(size)?) };
    Ok(())
}

fn at(addr: i64, offset: i64) -> Result<usize, String> {
    if addr == 0 {
        return Err("null pointer dereference".to_string());
    }
    addr.checked_add(offset)
        .and_then(|effective| usize::try_from(effective).ok())
        .ok_or_else(|| "pointer offset out of range".to_string())
}

pub fn span(addr: i64, offset: i64, width: usize) -> Result<usize, String> {
    let effective = at(addr, offset)?;
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
        Err(format!(
            "access of {width} bytes at offset {offset} is outside the {size}-byte allocation"
        ))
    }
}

pub fn cstring_bounds(addr: i64, offset: i64) -> Result<(usize, usize), String> {
    let start = at(addr, offset)?;
    let map = owned();
    let limit = match containing(&map, addr as usize) {
        Some((base, size)) if start >= base && start < base + size => base + size - start,
        Some(_) => return Err("offset is outside the allocation".to_string()),
        None => FOREIGN_CSTRING_SCAN,
    };
    Ok((start, limit))
}
