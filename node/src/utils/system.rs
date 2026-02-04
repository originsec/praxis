use std::fs;
use std::path::PathBuf;
use sysinfo::System;

pub fn get_machine_name() -> String {
    System::host_name().unwrap_or_else(|| "unknown".to_string())
}

pub fn get_os_details() -> String {
    let name = System::name().unwrap_or_else(|| "Unknown".to_string());
    let version = System::os_version().unwrap_or_else(|| "".to_string());
    let arch = System::cpu_arch();
    format!("{} {} ({})", name, version, arch)
}

//
// Get the praxis data directory (~/.local/share/praxis on Linux,
// %LOCALAPPDATA%\praxis on Windows).
//

fn get_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("praxis"))
}

//
// Get or create a persistent node ID. The ID is stored in the praxis data
// directory and persists across restarts.
//

pub fn get_or_create_node_id() -> String {
    let data_dir = match get_data_dir() {
        Some(dir) => dir,
        None => return uuid::Uuid::new_v4().to_string(),
    };

    let node_id_path = data_dir.join("node_id");

    //
    // Try to read existing node ID.
    //

    if let Ok(id) = fs::read_to_string(&node_id_path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }

    //
    // Generate new node ID and save it.
    //

    let node_id = uuid::Uuid::new_v4().to_string();

    if let Err(_) = fs::create_dir_all(&data_dir) {
        return node_id;
    }

    let _ = fs::write(&node_id_path, &node_id);

    node_id
}
