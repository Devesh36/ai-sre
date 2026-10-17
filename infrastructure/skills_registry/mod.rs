//! Immutable content-addressed workflow releases with transactional activation and rollback.
pub mod client;
use crate::{
    config::Workflow,
    core::agent_harness::skills::{Release, digest},
    store::Store,
};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct Registry {
    store: Store,
}
impl Registry {
    pub fn new(store: Store) -> Result<Self> {
        store.0.lock().map_err(|_|anyhow::anyhow!("database lock poisoned"))?.execute_batch(
            "CREATE TABLE IF NOT EXISTS workflow_releases(name TEXT NOT NULL,digest TEXT NOT NULL,revision INTEGER NOT NULL,document TEXT NOT NULL,PRIMARY KEY(name,digest),UNIQUE(name,revision));
             CREATE TABLE IF NOT EXISTS workflow_heads(name TEXT PRIMARY KEY,digest TEXT NOT NULL);")?;
        Ok(Self { store })
    }
    pub fn publish(&self, workflow: Workflow) -> Result<Release> {
        let digest = digest(&workflow)?;
        let mut conn = self
            .store
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(document) = tx
            .query_row(
                "SELECT document FROM workflow_releases WHERE name=?1 AND digest=?2",
                params![workflow.name, digest],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(serde_json::from_str(&document)?);
        }
        let revision: u64 = tx.query_row(
            "SELECT COALESCE(MAX(revision),0)+1 FROM workflow_releases WHERE name=?1",
            [&workflow.name],
            |r| r.get(0),
        )?;
        ensure!(revision <= 100, "workflow reached its 100-release limit");
        let release = Release {
            digest,
            revision,
            created_at: chrono::Utc::now(),
            workflow,
        };
        tx.execute(
            "INSERT INTO workflow_releases VALUES (?1,?2,?3,?4)",
            params![
                release.workflow.name,
                release.digest,
                release.revision,
                serde_json::to_string(&release)?
            ],
        )?;
        tx.commit()?;
        Ok(release)
    }
    pub fn active(&self, name: &str) -> Result<Release> {
        let conn = self
            .store
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let document:String=conn.query_row("SELECT r.document FROM workflow_releases r JOIN workflow_heads h ON r.name=h.name AND r.digest=h.digest WHERE h.name=?1",[name],|r|r.get(0)).optional()?.ok_or_else(||anyhow::anyhow!("workflow has no active release"))?;
        Ok(serde_json::from_str(&document)?)
    }
    pub fn get(&self, name: &str, digest: &str) -> Result<Release> {
        let conn = self
            .store
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let document: String = conn
            .query_row(
                "SELECT document FROM workflow_releases WHERE name=?1 AND digest=?2",
                params![name, digest],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("unknown workflow release"))?;
        Ok(serde_json::from_str(&document)?)
    }
    pub fn activate(&self, name: &str, digest: &str) -> Result<Release> {
        let mut conn = self
            .store
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let document: String = tx
            .query_row(
                "SELECT document FROM workflow_releases WHERE name=?1 AND digest=?2",
                params![name, digest],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("unknown workflow release"))?;
        tx.execute("INSERT INTO workflow_heads VALUES (?1,?2) ON CONFLICT(name) DO UPDATE SET digest=excluded.digest",params![name,digest])?;
        tx.commit()?;
        Ok(serde_json::from_str(&document)?)
    }
    pub fn rollback(&self, name: &str) -> Result<Release> {
        let mut conn = self
            .store
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let document:String=tx.query_row("SELECT r.document FROM workflow_releases r WHERE r.name=?1 AND r.revision < (SELECT active.revision FROM workflow_releases active JOIN workflow_heads h ON active.name=h.name AND active.digest=h.digest WHERE h.name=?1) ORDER BY r.revision DESC LIMIT 1",[name],|r|r.get(0)).optional()?.ok_or_else(||anyhow::anyhow!("workflow has no previous release"))?;
        let release: Release = serde_json::from_str(&document)?;
        tx.execute(
            "UPDATE workflow_heads SET digest=?1 WHERE name=?2",
            params![release.digest, name],
        )?;
        tx.commit()?;
        Ok(release)
    }
    pub fn list(&self) -> Result<Vec<Value>> {
        let conn = self
            .store
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let mut stmt=conn.prepare("SELECT r.name,MAX(r.revision),h.digest FROM workflow_releases r LEFT JOIN workflow_heads h ON h.name=r.name GROUP BY r.name ORDER BY r.name LIMIT 100")?;
        Ok(stmt.query_map([],|r|Ok(json!({"name":r.get::<_,String>(0)?,"releases":r.get::<_,u64>(1)?,"active_digest":r.get::<_,Option<String>>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn history(&self, name: &str) -> Result<Vec<Release>> {
        let conn = self
            .store
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT document FROM workflow_releases WHERE name=?1 ORDER BY revision DESC LIMIT 100",
        )?;
        let docs = stmt
            .query_map([name], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        docs.iter().map(|d| Ok(serde_json::from_str(d)?)).collect()
    }
}
