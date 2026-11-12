use rustc_hash::FxHashMap;

#[derive(Default)]
pub(super) struct Symbols {
    known: FxHashMap<usize, Option<String>>,
}

impl Symbols {
    pub(super) fn name(&mut self, address: usize) -> Option<&str> {
        self.known
            .entry(address)
            .or_insert_with(|| resolve(address).map(|raw| shorten(&raw)))
            .as_deref()
    }
}

fn shorten(raw: &str) -> String {
    let without_hash = match raw.rfind("::h") {
        Some(at)
            if raw.len() - at == 19 && raw[at + 3..].chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            &raw[..at]
        }
        Some(_) | None => raw,
    };
    without_hash.to_owned()
}

#[cfg(windows)]
fn resolve(address: usize) -> Option<String> {
    use std::sync::OnceLock;

    #[repr(C)]
    struct SymbolInfo {
        size_of_struct: u32,
        type_index: u32,
        reserved: [u64; 2],
        index: u32,
        size: u32,
        module_base: u64,
        flags: u32,
        value: u64,
        address: u64,
        register: u32,
        scope: u32,
        tag: u32,
        name_len: u32,
        max_name_len: u32,
        name: [u8; 1],
    }

    const NAME_CAPACITY: usize = 512;
    const SYMOPT_UNDNAME: u32 = 0x0000_0002;
    const SYMOPT_DEFERRED_LOADS: u32 = 0x0000_0004;

    #[link(name = "dbghelp")]
    unsafe extern "system" {
        fn SymSetOptions(options: u32) -> u32;
        fn SymInitialize(process: isize, search_path: *const u8, invade: i32) -> i32;
        fn SymLoadModuleExW(
            process: isize,
            file: isize,
            image: *const u16,
            module: *const u16,
            base: u64,
            size: u32,
            data: *const u8,
            flags: u32,
        ) -> u64;
        fn SymFromAddr(
            process: isize,
            address: u64,
            displacement: *mut u64,
            symbol: *mut SymbolInfo,
        ) -> i32;
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
    }

    static READY: OnceLock<bool> = OnceLock::new();
    let process = unsafe { GetCurrentProcess() };
    let ready = *READY.get_or_init(|| unsafe {
        SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS);
        let directory = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let mut path = format!("{directory};{directory}\\deps").into_bytes();
        path.push(0);
        SymInitialize(process, path.as_ptr(), 0) != 0
    });
    if !ready {
        return None;
    }

    let mut storage = vec![0u8; std::mem::size_of::<SymbolInfo>() + NAME_CAPACITY];
    let symbol = storage.as_mut_ptr().cast::<SymbolInfo>();
    unsafe {
        (*symbol).size_of_struct = std::mem::size_of::<SymbolInfo>() as u32;
        (*symbol).max_name_len = NAME_CAPACITY as u32;
    }
    let mut displacement = 0u64;
    let module = module_of(address)?;
    if module == own_module() {
        let image = loaded_image(address)?;
        static LOADED: OnceLock<u64> = OnceLock::new();
        LOADED.get_or_init(|| unsafe {
            SymLoadModuleExW(
                process,
                0,
                image.path.as_ptr(),
                std::ptr::null(),
                image.base as u64,
                image.size,
                std::ptr::null(),
                0,
            )
        });
    }
    let found = unsafe { SymFromAddr(process, address as u64, &mut displacement, symbol) };
    if found == 0 || module != own_module() {
        return Some(format!("<{module}>"));
    }
    let length = unsafe { (*symbol).name_len } as usize;
    let name = unsafe {
        std::slice::from_raw_parts(std::ptr::addr_of!((*symbol).name).cast::<u8>(), length)
    };
    Some(String::from_utf8_lossy(name).into_owned())
}

#[cfg(windows)]
struct LoadedImage {
    path: Vec<u16>,
    base: usize,
    size: u32,
}

#[cfg(windows)]
fn loaded_image(address: usize) -> Option<LoadedImage> {
    const FROM_ADDRESS_UNCHANGED: u32 = 0x0000_0004;
    unsafe extern "system" {
        fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut isize) -> i32;
        fn GetModuleFileNameW(module: isize, buffer: *mut u16, size: u32) -> u32;
    }
    let mut handle = 0isize;
    if unsafe { GetModuleHandleExW(FROM_ADDRESS_UNCHANGED, address as *const u16, &mut handle) }
        == 0
    {
        return None;
    }
    let mut buffer = vec![0u16; 1024];
    let length =
        unsafe { GetModuleFileNameW(handle, buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    buffer.truncate(length);
    buffer.push(0);
    let base = handle as usize;
    let header_offset = unsafe { *((base + 0x3c) as *const u32) } as usize;
    let size = unsafe { *((base + header_offset + 24 + 56) as *const u32) };
    Some(LoadedImage {
        path: buffer,
        base,
        size,
    })
}

#[cfg(windows)]
fn module_of(address: usize) -> Option<String> {
    const FROM_ADDRESS_UNCHANGED: u32 = 0x0000_0004;
    unsafe extern "system" {
        fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut isize) -> i32;
        fn GetModuleFileNameW(module: isize, buffer: *mut u16, size: u32) -> u32;
    }
    let mut handle = 0isize;
    if unsafe { GetModuleHandleExW(FROM_ADDRESS_UNCHANGED, address as *const u16, &mut handle) }
        == 0
    {
        return None;
    }
    let mut buffer = [0u16; 520];
    let length =
        unsafe { GetModuleFileNameW(handle, buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    let path = String::from_utf16_lossy(&buffer[..length]);
    path.rsplit(['\\', '/']).next().map(str::to_owned)
}

#[cfg(windows)]
fn own_module() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

#[cfg(unix)]
fn resolve(address: usize) -> Option<String> {
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    if unsafe { libc::dladdr(address as *const libc::c_void, &mut info) } == 0
        || info.dli_sname.is_null()
    {
        return None;
    }
    let name = unsafe { std::ffi::CStr::from_ptr(info.dli_sname) };
    Some(name.to_string_lossy().into_owned())
}

#[cfg(not(any(windows, unix)))]
fn resolve(_: usize) -> Option<String> {
    None
}
