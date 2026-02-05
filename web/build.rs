//! Build script for praxis_web
//!
//! Builds the frontend if not already built, or if source files changed.

use std::env;
use std::path::Path;
use std::process::Command;

fn main() -> anyhow::Result<()> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let frontend_dir = Path::new(&manifest_dir).join("frontend");
    let dist_dir = frontend_dir.join("dist");

    //
    // Rerun if frontend source changes.
    //
    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/index.html");
    println!("cargo:rerun-if-changed=frontend/package.json");
    println!("cargo:rerun-if-changed=frontend/vite.config.ts");
    println!("cargo:rerun-if-changed=frontend/tsconfig.json");

    //
    // Check if we should skip frontend build (for CI or quick iteration).
    //
    if env::var("PRAXIS_SKIP_FRONTEND").is_ok() {
        println!("cargo:warning=Skipping frontend build (PRAXIS_SKIP_FRONTEND set)");
        return Ok(());
    }

    //
    // Check if dist exists and has content.
    //
    if dist_dir.exists() && dist_dir.join("index.html").exists() {
        //
        // In release mode, always rebuild to ensure freshness
        // In debug mode, skip if dist exists.
        //
        if env::var("PROFILE").unwrap_or_default() != "release" {
            println!("cargo:warning=Frontend dist exists, skipping build (debug mode)");
            return Ok(());
        }
    }

    //
    // Check for npm.
    //
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };

    //
    // Check if node_modules exists, if not run npm install.
    //
    if !frontend_dir.join("node_modules").exists() {
        println!("cargo:warning=Installing frontend dependencies...");
        let status = Command::new(npm)
            .current_dir(&frontend_dir)
            .args(["install"])
            .status();

        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                println!("cargo:warning=npm install failed with status: {}", s);
                //
                // Don't fail the build, just warn.
                //
                return Ok(());
            }
            Err(e) => {
                println!("cargo:warning=npm not found or failed: {}", e);
                println!("cargo:warning=Frontend will not be embedded. Install Node.js to build frontend.");
                return Ok(());
            }
        }
    }

    //
    // Build frontend.
    //
    println!("cargo:warning=Building frontend...");
    let status = Command::new(npm)
        .current_dir(&frontend_dir)
        .args(["run", "build"])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=Frontend build complete");
        }
        Ok(s) => {
            println!("cargo:warning=Frontend build failed with status: {}", s);
        }
        Err(e) => {
            println!("cargo:warning=Failed to run npm build: {}", e);
        }
    }

    Ok(())
}
