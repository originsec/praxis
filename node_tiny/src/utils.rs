use std::fs;
use std::path::PathBuf;
use sysinfo::System;

#[cfg(unix)]
pub fn is_privileged() -> bool {
    nix::unistd::geteuid().is_root()
}

#[cfg(windows)]
pub fn is_privileged() -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size,
            &mut ret_len,
        );
        let _ = windows::Win32::Foundation::CloseHandle(token);
        ok.is_ok() && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(any(unix, windows)))]
pub fn is_privileged() -> bool {
    false
}

pub fn get_machine_name() -> String {
    System::host_name().unwrap_or_else(|| "unknown".to_string())
}

pub fn get_os_details() -> String {
    let name = System::name().unwrap_or_else(|| "Unknown".to_string());
    let version = System::os_version().unwrap_or_else(|| "".to_string());
    let arch = System::cpu_arch();
    format!("{} {} ({})", name, version, arch)
}

fn get_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("praxis"))
}

pub fn get_or_create_node_id() -> String {
    let data_dir = match get_data_dir() {
        Some(dir) => dir,
        None => return uuid::Uuid::new_v4().to_string(),
    };

    let node_id_path = data_dir.join("node_id");

    if let Ok(id) = fs::read_to_string(&node_id_path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }

    let node_id = uuid::Uuid::new_v4().to_string();
    if fs::create_dir_all(&data_dir).is_err() {
        return node_id;
    }
    let _ = fs::write(&node_id_path, &node_id);
    node_id
}
