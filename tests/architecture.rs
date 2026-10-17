use ai_sre::bootstrap::architecture::Inventory;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[test]
fn inventory_paths_and_implementation_pointers_are_consistent() {
    let inventory = Inventory::bundled().unwrap();
    assert_eq!(inventory.files.len(), 4385);
    assert_eq!(inventory.directories.len(), 755);
    assert_eq!(inventory.declaration_scan_failures, 0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut paths = BTreeSet::new();
    for entry in &inventory.files {
        assert!(paths.insert(&entry.path), "duplicate {}", entry.path);
        assert!(!entry.path.starts_with('/') && !entry.path.split('/').any(|p| p == ".."));
        if entry.rust_target.is_some() {
            assert!(matches!(
                entry.status.as_deref(),
                Some("partial" | "pending")
            ));
        }
        for target in &entry.related_rust {
            assert!(
                root.join(target).is_file(),
                "missing {target} for {}",
                entry.path
            );
        }
    }
    assert!(inventory.select("../core", false).is_err());
    let selected = inventory.select("core/agent_harness", false).unwrap();
    assert!(!selected.is_empty());
    assert!(!inventory.select("", true).unwrap().is_empty());
    assert!(
        selected
            .iter()
            .all(|e| e.path.starts_with("core/agent_harness/"))
    );
}

fn collect(path: &Path, files: &mut BTreeSet<PathBuf>) {
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.insert(path.canonicalize().unwrap());
        }
    }
}
fn walk(file: &Path, visited: &mut BTreeSet<PathBuf>) {
    let file = file.canonicalize().unwrap();
    if !visited.insert(file.clone()) {
        return;
    }
    let source = std::fs::read_to_string(&file).unwrap();
    let re = regex::Regex::new(
        r#"(?m)(?:#\[path\s*=\s*"([^"]+)"\]\s*)?(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;"#,
    )
    .unwrap();
    for captures in re.captures_iter(&source) {
        let parent = file.parent().unwrap();
        let child = if let Some(explicit) = captures.get(1) {
            parent.join(explicit.as_str())
        } else {
            let stem = file.file_stem().unwrap().to_str().unwrap();
            let base = if matches!(stem, "lib" | "mod" | "main" | "entrypoint") {
                parent.to_owned()
            } else {
                parent.join(stem)
            };
            let flat = base.join(format!("{}.rs", &captures[2]));
            if flat.is_file() {
                flat
            } else {
                base.join(&captures[2]).join("mod.rs")
            }
        };
        walk(&child, visited);
    }
}
#[test]
fn every_owned_rust_module_is_reachable_from_a_compiled_entrypoint() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = BTreeSet::new();
    for name in [
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
        collect(&root.join(name), &mut files);
    }
    let mut visited = BTreeSet::new();
    walk(&root.join("src/lib.rs"), &mut visited);
    walk(&root.join("surfaces/entrypoint.rs"), &mut visited);
    assert_eq!(files, visited, "orphaned implementation module");
}
