use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path)
        .expect("source directory")
        .map(Result::unwrap)
    {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files)
        } else if path.extension().is_some_and(|s| s == "rs") {
            files.push(path)
        }
    }
}
fn main() {
    let mut files = vec![
        PathBuf::from("Cargo.lock"),
        PathBuf::from("Cargo.toml"),
        PathBuf::from("build.rs"),
        PathBuf::from("docs/module-inventory.json"),
    ];
    for root in [
        "src",
        "bootstrap",
        "config",
        "core",
        "gateway",
        "infrastructure",
        "integrations",
        "surfaces",
        "tools",
    ] {
        println!("cargo:rerun-if-changed={root}");
        collect(Path::new(root), &mut files);
    }
    files.sort();
    let mut digest = Sha256::new();
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        digest.update(path.to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(fs::read(path).expect("source file"));
        digest.update([0]);
    }
    println!(
        "cargo:rustc-env=AI_SRE_IMPLEMENTATION_SHA256={:x}",
        digest.finalize()
    );
}
