use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let scripts_dir = Path::new(&manifest_dir).parent().unwrap().join("agents");

    println!("cargo:rerun-if-changed={}", scripts_dir.display());

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("embedded_lua.rs");
    let mut out = fs::File::create(&dest).unwrap();

    let profile = env::var("PROFILE").unwrap_or_default();

    writeln!(out, "pub const EMBEDDED_LUA_SCRIPTS: &[&str] = &[").unwrap();

    //
    // Only embed Lua scripts in debug builds. Release builds get scripts
    // from the service exclusively.
    //
    if profile == "debug" {
        let mut entries: Vec<String> = Vec::new();

        if scripts_dir.exists() {
            for entry in fs::read_dir(&scripts_dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("lua") {
                    println!("cargo:rerun-if-changed={}", path.display());
                    let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                    entries.push(file_name);
                }
            }
        }

        entries.sort();

        for file_name in &entries {
            writeln!(
                out,
                "    include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../agents/{}\")),",
                file_name
            )
            .unwrap();
        }
    }

    writeln!(out, "];").unwrap();
}
