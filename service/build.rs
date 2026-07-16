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

    let mut entries: Vec<(String, String)> = Vec::new();

    if scripts_dir.exists() {
        for entry in fs::read_dir(&scripts_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("lua") {
                println!("cargo:rerun-if-changed={}", path.display());
                let stem = path.file_stem().unwrap().to_string_lossy().to_string();
                let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                entries.push((stem, file_name));
            }
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    writeln!(out, "pub const EMBEDDED_LUA_SCRIPTS: &[(&str, &str)] = &[").unwrap();
    for (stem, file_name) in &entries {
        writeln!(
            out,
            "    (\"{}\", include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../agents/{}\"))),",
            stem, file_name
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();

    writeln!(
        out,
        "pub const EMBEDDED_LUA_SCRIPTS_VERSION: &str = env!(\"CARGO_PKG_VERSION\");"
    )
    .unwrap();

    embed_docs(&manifest_dir, &out_dir);
}

//
// Embed the mdBook documentation sources (docs/src/**/*.md) into the binary
// so the documentation helper agent can be seeded with them at runtime with
// no filesystem dependency. Emits a flat list of (relative_path, contents)
// pairs; runtime chunking/retrieval happens in the doc_helper module.
//
fn embed_docs(manifest_dir: &str, out_dir: &str) {
    let docs_dir = Path::new(manifest_dir)
        .parent()
        .unwrap()
        .join("docs")
        .join("src");

    println!("cargo:rerun-if-changed={}", docs_dir.display());

    let dest = Path::new(out_dir).join("embedded_docs.rs");
    let mut out = fs::File::create(&dest).unwrap();

    let mut docs: Vec<String> = Vec::new();
    if docs_dir.exists() {
        collect_markdown(&docs_dir, &docs_dir, &mut docs);
    }
    docs.sort();

    writeln!(out, "pub const EMBEDDED_DOCS: &[(&str, &str)] = &[").unwrap();
    for rel in &docs {
        writeln!(
            out,
            "    (\"{0}\", include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../docs/src/{0}\"))),",
            rel
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
}

//
// Recursively collect *.md files under `root`, recording each path relative
// to `base` with forward-slash separators (so the generated include_str!
// paths are portable across platforms).
//
fn collect_markdown(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown(base, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            println!("cargo:rerun-if-changed={}", path.display());
            if let Ok(rel) = path.strip_prefix(base) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.push(rel);
            }
        }
    }
}
