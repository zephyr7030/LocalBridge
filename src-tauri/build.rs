use std::{env, fs, path::PathBuf, process::Command, time::SystemTime};

fn ensure_dummy_sidecar() {
    let target = env::var("TARGET").unwrap_or_default();
    if target != "x86_64-pc-windows-msvc" {
        panic!("LB-001 supports only x86_64-pc-windows-msvc, got {target}");
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let source = manifest_dir.join("src/bin/dummy-sidecar.rs");
    let binaries = manifest_dir.join("binaries");
    let output = binaries.join("dummy-sidecar-x86_64-pc-windows-msvc.exe");
    println!("cargo:rerun-if-changed={}", source.display());

    let modified = |path: &PathBuf| -> Option<SystemTime> {
        fs::metadata(path).ok()?.modified().ok()
    };
    let needs_build = match (modified(&source), modified(&output)) {
        (Some(src), Some(out)) => src > out,
        (Some(_), None) => true,
        _ => true,
    };
    if !needs_build {
        return;
    }

    fs::create_dir_all(&binaries).expect("create dummy sidecar directory");
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let status = Command::new(rustc)
        .arg(&source)
        .args(["-O", "-o"])
        .arg(&output)
        .status()
        .expect("launch rustc for LB-001 dummy sidecar");
    assert!(status.success(), "compile LB-001 dummy sidecar");
}

fn main() {
    ensure_dummy_sidecar();
    tauri_build::build();
}
