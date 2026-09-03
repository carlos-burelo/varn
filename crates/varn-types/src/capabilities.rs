use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CAP_FS_READ: u64 = 1 << 0;
pub const CAP_FS_WRITE: u64 = 1 << 1;
pub const CAP_NET_CLIENT: u64 = 1 << 2;
pub const CAP_NET_SERVER: u64 = 1 << 3;
pub const CAP_SYS_ENV: u64 = 1 << 4;
pub const CAP_SYS_EXEC: u64 = 1 << 5;
pub const CAP_SYS_FFI: u64 = 1 << 6;
pub const CAP_ALL: u64 = u64::MAX;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    /// Bitmask fast-path: 1 CPU instruction check for unrestricted rights
    pub mask: u64,

    /// Whitelist for fs:read paths (None = any path allowed if CAP_FS_READ is set)
    pub fs_read_paths: Option<Vec<PathBuf>>,

    /// Whitelist for fs:write paths (None = any path allowed if CAP_FS_WRITE is set)
    pub fs_write_paths: Option<Vec<PathBuf>>,

    /// Whitelist for outgoing network domains/hosts (None = any host allowed)
    pub net_hosts: Option<Vec<String>>,

    /// Whitelist for listening ports (None = any port allowed)
    pub net_ports: Option<Vec<i64>>,

    /// Whitelist for environment variables (None = any env var allowed)
    pub env_vars: Option<Vec<String>>,
}

impl Default for CapabilitySet {
    fn default() -> Self {
        Self::allow_all()
    }
}

impl CapabilitySet {
    pub const fn allow_all() -> Self {
        Self {
            mask: CAP_ALL,
            fs_read_paths: None,
            fs_write_paths: None,
            net_hosts: None,
            net_ports: None,
            env_vars: None,
        }
    }

    pub const fn sandbox() -> Self {
        Self {
            mask: 0,
            fs_read_paths: None,
            fs_write_paths: None,
            net_hosts: None,
            net_ports: None,
            env_vars: None,
        }
    }

    #[inline(always)]
    pub fn has_mask(&self, flag: u64) -> bool {
        (self.mask & flag) == flag
    }

    pub fn has_capability(&self, cap: &str) -> bool {
        match cap {
            "fs.read" | "fs:read" => self.has_mask(CAP_FS_READ),
            "fs.write" | "fs:write" => self.has_mask(CAP_FS_WRITE),
            "net.client" | "net:client" | "net.connect" => self.has_mask(CAP_NET_CLIENT),
            "net.server" | "net:server" | "net.listen" => self.has_mask(CAP_NET_SERVER),
            "sys.env" | "sys:env" => self.has_mask(CAP_SYS_ENV),
            "sys.exec" | "sys:exec" => self.has_mask(CAP_SYS_EXEC),
            "sys.ffi" | "sys:ffi" => self.has_mask(CAP_SYS_FFI),
            _ => true,
        }
    }

    pub fn check_fs_read(&self, path: &Path) -> bool {
        if !self.has_mask(CAP_FS_READ) {
            return false;
        }
        let Some(ref allowed_paths) = self.fs_read_paths else {
            return true;
        };
        path_matches_any(path, allowed_paths)
    }

    pub fn check_fs_write(&self, path: &Path) -> bool {
        if !self.has_mask(CAP_FS_WRITE) {
            return false;
        }
        let Some(ref allowed_paths) = self.fs_write_paths else {
            return true;
        };
        path_matches_any(path, allowed_paths)
    }

    pub fn check_net_connect(&self, host: &str) -> bool {
        if !self.has_mask(CAP_NET_CLIENT) {
            return false;
        }
        let Some(ref allowed_hosts) = self.net_hosts else {
            return true;
        };
        allowed_hosts.iter().any(|allowed| {
            allowed == "*" || allowed.eq_ignore_ascii_case(host) || host.ends_with(allowed.as_str())
        })
    }

    pub fn check_net_listen(&self, port: i64) -> bool {
        if !self.has_mask(CAP_NET_SERVER) {
            return false;
        }
        let Some(ref allowed_ports) = self.net_ports else {
            return true;
        };
        allowed_ports.contains(&port)
    }

    pub fn check_env(&self, key: &str) -> bool {
        if !self.has_mask(CAP_SYS_ENV) {
            return false;
        }
        let Some(ref allowed_vars) = self.env_vars else {
            return true;
        };
        allowed_vars
            .iter()
            .any(|v| v == "*" || v.eq_ignore_ascii_case(key))
    }

    #[inline(always)]
    pub fn check_ffi(&self) -> bool {
        self.has_mask(CAP_SYS_FFI)
    }

    #[inline(always)]
    pub fn check_exec(&self) -> bool {
        self.has_mask(CAP_SYS_EXEC)
    }
}

fn path_matches_any(path: &Path, allowed: &[PathBuf]) -> bool {
    let Ok(canonical) = path
        .canonicalize()
        .or_else(|_| Ok::<_, ()>(path.to_path_buf()))
    else {
        return false;
    };
    for prefix in allowed {
        let Ok(canonical_prefix) = prefix
            .canonicalize()
            .or_else(|_| Ok::<_, ()>(prefix.clone()))
        else {
            continue;
        };
        if canonical.starts_with(&canonical_prefix) {
            return true;
        }
    }
    false
}
