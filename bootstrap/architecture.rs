//! Developer-facing module inventory. Planned paths never imply implemented code.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub struct Inventory {
    pub schema_version: u32,
    pub scope: String,
    pub snapshot: String,
    pub directories: Vec<String>,
    pub files: Vec<Entry>,
    pub declaration_scan_failures: usize,
}
#[derive(Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub kind: String,
    pub bytes: usize,
    pub lines: Option<usize>,
    pub rust_target: Option<String>,
    #[serde(default)]
    pub declarations: Vec<Declaration>,
    #[serde(default)]
    pub related_rust: Vec<String>,
    pub status: Option<String>,
    pub declaration_scan: Option<String>,
    pub translation: Option<Translation>,
}
#[derive(Serialize, Deserialize)]
pub struct Translation {
    pub state: String,
    pub targets: Vec<String>,
    pub validation: Vec<String>,
    pub differences_and_remaining: Vec<String>,
}
#[derive(Serialize, Deserialize)]
pub struct Declaration {
    pub name: String,
    pub kind: String,
}
impl Inventory {
    pub fn bundled() -> Result<Self> {
        Ok(serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/module-inventory.json"
        )))?)
    }
    pub fn summary(&self) -> serde_json::Value {
        let mut layers: BTreeMap<&str, usize> = BTreeMap::new();
        for entry in &self.files {
            if entry.rust_target.is_some() {
                *layers
                    .entry(entry.path.split('/').next().unwrap_or("root"))
                    .or_default() += 1;
            }
        }
        serde_json::json!({"directories":self.directories.len(),"files":self.files.len(),"modules":self.files.iter().filter(|e|e.rust_target.is_some()).count(),"partial":self.files.iter().filter(|e|e.status.as_deref()==Some("partial")).count(),"pending":self.files.iter().filter(|e|e.status.as_deref()==Some("pending")).count(),"translation_records":self.files.iter().filter(|e|e.translation.is_some()).count(),"declarations":self.files.iter().map(|e|e.declarations.len()).sum::<usize>(),"layers":layers,"scope":self.scope})
    }
    pub fn select(&self, prefix: &str, pending: bool) -> Result<Vec<&Entry>> {
        ensure!(
            !prefix.contains("..") && !prefix.starts_with('/'),
            "use a relative module prefix"
        );
        let prefix = prefix.trim_end_matches('/');
        Ok(self
            .files
            .iter()
            .filter(|e| {
                (prefix.is_empty() || e.path == prefix || e.path.starts_with(&format!("{prefix}/")))
                    && (!pending || e.status.as_deref() == Some("pending"))
            })
            .collect())
    }
}
