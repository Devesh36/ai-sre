use crate::{core::state::history::Entry, store::Store, tools::Toolbox};
use anyhow::{Result, ensure};
use rusqlite::params;
#[derive(Clone)]
pub struct History(Store);
impl History {
    pub fn new(store: Store) -> Result<Self> {
        store.0.lock().map_err(|_|anyhow::anyhow!("database lock poisoned"))?.execute_batch("CREATE TABLE IF NOT EXISTS shell_history(id INTEGER PRIMARY KEY AUTOINCREMENT,service TEXT NOT NULL,text TEXT NOT NULL,created_at TEXT NOT NULL)")?;
        Ok(Self(store))
    }
    pub fn append(&self, tools: &Toolbox, service: &str, text: &str) -> Result<()> {
        ensure!(
            !service.is_empty()
                && service.len() <= 128
                && !text.trim().is_empty()
                && text.len() <= 4000,
            "invalid history entry"
        );
        let text = tools.redact(text);
        let mut conn = self
            .0
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO shell_history(service,text,created_at) VALUES (?1,?2,?3)",
            params![service, text, chrono::Utc::now().to_rfc3339()],
        )?;
        tx.execute("DELETE FROM shell_history WHERE service=?1 AND id NOT IN(SELECT id FROM shell_history WHERE service=?1 ORDER BY id DESC LIMIT 200)",[service])?;
        tx.execute("DELETE FROM shell_history WHERE id NOT IN(SELECT id FROM shell_history ORDER BY id DESC LIMIT 5000)",[])?;
        tx.commit()?;
        Ok(())
    }
    pub fn list(&self, service: &str, limit: u32) -> Result<Vec<Entry>> {
        ensure!((1..=200).contains(&limit), "history limit must be 1–200");
        let conn = self
            .0
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let mut stmt=conn.prepare("SELECT id,service,text,created_at FROM shell_history WHERE service=?1 ORDER BY id DESC LIMIT ?2")?;
        Ok(stmt
            .query_map(params![service, limit], |r| {
                Ok(Entry {
                    id: r.get(0)?,
                    service: r.get(1)?,
                    text: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn clear(&self, service: &str) -> Result<usize> {
        Ok(self
            .0
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?
            .execute("DELETE FROM shell_history WHERE service=?1", [service])?)
    }
}
