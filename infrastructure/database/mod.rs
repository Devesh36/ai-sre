use crate::domain::{Incident, Status};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Clone)]
pub struct Store(pub(crate) Arc<Mutex<Connection>>);

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
            CREATE TABLE IF NOT EXISTS incidents (
                id TEXT PRIMARY KEY, dedup_key TEXT UNIQUE, updated_at TEXT NOT NULL, document TEXT NOT NULL
            ); CREATE INDEX IF NOT EXISTS incidents_updated ON incidents(updated_at DESC);
            CREATE INDEX IF NOT EXISTS incidents_service_created ON incidents(json_extract(document,'$.service'),json_extract(document,'$.created_at') DESC);
            CREATE TABLE IF NOT EXISTS records(kind TEXT NOT NULL,id TEXT NOT NULL,document TEXT NOT NULL,PRIMARY KEY(kind,id));
            CREATE TABLE IF NOT EXISTS schedules(id TEXT PRIMARY KEY,enabled INTEGER NOT NULL,next_at TEXT NOT NULL,document TEXT NOT NULL);")?;
        Ok(Self(Arc::new(Mutex::new(conn))))
    }

    pub fn insert(&self, incident: &Incident, key: Option<&str>) -> Result<(Incident, bool)> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let count = conn.execute(
            "INSERT INTO incidents VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
            params![
                incident.id.to_string(),
                key,
                incident.updated_at.to_rfc3339(),
                serde_json::to_string(incident)?
            ],
        )?;
        if count == 0 {
            let doc: String = conn.query_row(
                "SELECT document FROM incidents WHERE dedup_key = ?1 OR id = ?2",
                params![key, incident.id.to_string()],
                |r| r.get(0),
            )?;
            return Ok((serde_json::from_str(&doc)?, false));
        }
        Ok((incident.clone(), true))
    }

    pub fn get(&self, id: Uuid) -> Result<Option<Incident>> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let doc: Option<String> = conn
            .query_row(
                "SELECT document FROM incidents WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        doc.map(|d| serde_json::from_str(&d).context("decode incident"))
            .transpose()
    }

    pub fn list(&self) -> Result<Vec<Incident>> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let mut stmt =
            conn.prepare("SELECT document FROM incidents ORDER BY updated_at DESC LIMIT 100")?;
        let docs = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        docs.into_iter()
            .map(|d| serde_json::from_str(&d).context("decode incident"))
            .collect()
    }

    /// Filter before applying the scan bound: other services must not crowd out useful cases.
    pub fn recall(&self, incident: &Incident) -> Result<crate::domain::recall::Recall> {
        let candidates = {
            let conn = self
                .0
                .lock()
                .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
            let mut stmt = conn.prepare(
                "SELECT document FROM incidents
                WHERE json_extract(document,'$.service')=?1
                  AND json_extract(document,'$.created_at')<?2 AND id<>?3
                  AND json_extract(document,'$.status') IN ('triaged','resolved')
                ORDER BY json_extract(document,'$.created_at') DESC, id ASC LIMIT ?4",
            )?;
            let docs = stmt
                .query_map(
                    params![
                        incident.service,
                        incident.created_at.to_rfc3339(),
                        incident.id.to_string(),
                        (crate::domain::recall::SEARCH_LIMIT + 1) as u32
                    ],
                    |r| r.get::<_, String>(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            docs.into_iter()
                .map(|s| serde_json::from_str(&s).context("decode recall candidate"))
                .collect::<Result<Vec<Incident>>>()?
        };
        Ok(crate::domain::recall::analyze(
            incident,
            &candidates,
            chrono::Utc::now(),
        ))
    }

    /// A SQLite write transaction serializes claims across threads and processes.
    pub fn claim(&self, id: Uuid, question: Option<&str>) -> Result<Incident> {
        self.mutate(id, |incident| {
            if incident.status == Status::Investigating {
                bail!("investigation already running");
            }
            if incident.status == Status::Resolved {
                bail!("resolved incidents cannot be investigated");
            }
            if incident
                .events
                .iter()
                .filter(|e| e.kind == "investigation_started")
                .count()
                >= 20
            {
                bail!("incident has reached its 20-run limit; open a new investigation");
            }
            incident.status = Status::Investigating;
            incident.run_id = Some(Uuid::new_v4());
            incident.cancel_requested = false;
            incident.event(
                "investigation_started",
                question.unwrap_or("Collecting evidence"),
            );
            Ok(())
        })
    }

    pub fn mutate(
        &self,
        id: Uuid,
        f: impl FnOnce(&mut Incident) -> Result<()>,
    ) -> Result<Incident> {
        let mut conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let doc: String = tx.query_row(
            "SELECT document FROM incidents WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )?;
        let mut incident: Incident = serde_json::from_str(&doc)?;
        f(&mut incident)?;
        tx.execute(
            "UPDATE incidents SET updated_at=?1, document=?2 WHERE id=?3",
            params![
                incident.updated_at.to_rfc3339(),
                serde_json::to_string(&incident)?,
                id.to_string()
            ],
        )?;
        tx.commit()?;
        Ok(incident)
    }

    pub fn finish(&self, incident: &Incident) -> Result<Incident> {
        self.mutate(incident.id, |current| {
            anyhow::ensure!(
                current.status == Status::Investigating && current.run_id == incident.run_id,
                "investigation no longer owns this incident"
            );
            let cancel = current.cancel_requested;
            *current = incident.clone();
            if cancel && current.status != Status::Cancelled {
                current.status = Status::Cancelled;
                current.cancel_requested = true;
                if let Some(terminal) = current.trace.last_mut()
                    && terminal.run_id == current.run_id
                    && matches!(terminal.phase.as_str(), "run_completed" | "run_failed")
                {
                    terminal.phase = "run_cancelled".into();
                }
                current.event(
                    "investigation_cancelled",
                    "Cancellation requested before the final commit",
                );
            }
            Ok(())
        })
    }

    pub fn checkpoint(&self, incident: &Incident) -> Result<()> {
        self.mutate(incident.id, |current| {
            anyhow::ensure!(
                current.status == Status::Investigating && current.run_id == incident.run_id,
                "run ownership changed"
            );
            let cancel = current.cancel_requested;
            *current = incident.clone();
            current.cancel_requested = cancel;
            Ok(())
        })?;
        Ok(())
    }

    pub fn cancel(&self, id: Uuid) -> Result<Incident> {
        self.mutate(id, |i| {
            anyhow::ensure!(
                i.status == Status::Investigating,
                "incident is not investigating"
            );
            i.cancel_requested = true;
            Ok(())
        })
    }

    pub fn compact(&self, id: Uuid) -> Result<Incident> {
        self.mutate(id,|i|{
            anyhow::ensure!(i.status!=Status::Investigating,"cannot compact an active run");
            i.context_summary=Some(i.report.as_ref().map(|r|format!("{}\nGaps: {}",r.summary,r.gaps.join("; "))).unwrap_or_else(||i.title.clone()));
            i.event("context_compacted","Future model prompts use the summary, recent history, and current-run evidence; original evidence remains retained");
            Ok(())
        })
    }

    pub fn record_put(&self, kind: &str, id: &str, value: &serde_json::Value) -> Result<()> {
        anyhow::ensure!(
            matches!(
                kind,
                "memory" | "work" | "fleet" | "action" | "workflow_remote"
            ),
            "unknown record kind"
        );
        let doc = serde_json::to_string(value)?;
        anyhow::ensure!(doc.len() <= 65536, "record exceeds 64 KiB");
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        conn.execute("INSERT INTO records VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET document=excluded.document",params![kind,id,doc])?;
        Ok(())
    }

    pub fn records(&self, kind: &str) -> Result<Vec<serde_json::Value>> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let mut stmt = conn
            .prepare("SELECT document FROM records WHERE kind=?1 ORDER BY rowid DESC LIMIT 100")?;
        let rows = stmt
            .query_map([kind], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }

    pub fn record_delete(&self, kind: &str, id: &str) -> Result<bool> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        Ok(conn.execute(
            "DELETE FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
        )? > 0)
    }

    pub fn record_get(&self, kind: &str, id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let doc: Option<String> = conn
            .query_row(
                "SELECT document FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                |r| r.get(0),
            )
            .optional()?;
        doc.map(|d| Ok(serde_json::from_str(&d)?)).transpose()
    }

    pub fn service_records(&self, kind: &str, service: &str) -> Result<Vec<serde_json::Value>> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let mut stmt=conn.prepare("SELECT document FROM records WHERE kind=?1 AND json_extract(document,'$.service')=?2 ORDER BY rowid DESC LIMIT 100")?;
        let rows = stmt
            .query_map(params![kind, service], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }

    pub fn record_mutate(
        &self,
        kind: &str,
        id: &str,
        f: impl FnOnce(&mut serde_json::Value) -> Result<()>,
    ) -> Result<serde_json::Value> {
        let mut conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let doc: String = tx.query_row(
            "SELECT document FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
            |r| r.get(0),
        )?;
        let mut value = serde_json::from_str(&doc)?;
        f(&mut value)?;
        let doc = serde_json::to_string(&value)?;
        anyhow::ensure!(doc.len() <= 65536, "record exceeds 64 KiB");
        tx.execute(
            "UPDATE records SET document=?1 WHERE kind=?2 AND id=?3",
            params![doc, kind, id],
        )?;
        tx.commit()?;
        Ok(value)
    }

    pub fn schedule_add(&self, s: &crate::operations::Schedule) -> Result<()> {
        crate::operations::next_occurrence(&s.expression, &s.timezone, chrono::Utc::now())?;
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        conn.execute(
            "INSERT INTO schedules VALUES(?1,?2,?3,?4)",
            params![
                s.id.to_string(),
                s.enabled,
                s.next_at.to_rfc3339(),
                serde_json::to_string(s)?
            ],
        )?;
        Ok(())
    }

    pub fn schedules(&self) -> Result<Vec<crate::operations::Schedule>> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT document FROM schedules ORDER BY next_at LIMIT 100")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }

    pub fn schedule_remove(&self, id: Uuid) -> Result<bool> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        Ok(conn.execute("DELETE FROM schedules WHERE id=?1", [id.to_string()])? > 0)
    }

    /// Advancing the schedule and enqueuing its incident are one transaction.
    pub fn enqueue_due(&self, now: chrono::DateTime<chrono::Utc>) -> Result<usize> {
        let mut conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let rows = {
            let mut stmt = tx.prepare(
                "SELECT document FROM schedules WHERE enabled=1 AND next_at<=?1 LIMIT 20",
            )?;
            stmt.query_map([now.to_rfc3339()], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let count = rows.len();
        for doc in rows {
            let mut schedule: crate::operations::Schedule = serde_json::from_str(&doc)?;
            let mut incident = Incident::new(schedule.question.clone(), schedule.service.clone())?;
            incident.schedule_id = Some(schedule.id);
            incident.event(
                "scheduled",
                format!("Scheduled occurrence at {}", schedule.next_at),
            );
            let key = format!("schedule:{}:{}", schedule.id, schedule.next_at.to_rfc3339());
            tx.execute(
                "INSERT INTO incidents VALUES(?1,?2,?3,?4) ON CONFLICT DO NOTHING",
                params![
                    incident.id.to_string(),
                    key,
                    incident.updated_at.to_rfc3339(),
                    serde_json::to_string(&incident)?
                ],
            )?;
            schedule.next_at =
                crate::operations::next_occurrence(&schedule.expression, &schedule.timezone, now)?;
            tx.execute(
                "UPDATE schedules SET next_at=?1,document=?2 WHERE id=?3",
                params![
                    schedule.next_at.to_rfc3339(),
                    serde_json::to_string(&schedule)?,
                    schedule.id.to_string()
                ],
            )?;
        }
        tx.commit()?;
        Ok(count)
    }

    pub fn queued(&self) -> Result<Vec<Incident>> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let mut stmt=conn.prepare("SELECT document FROM incidents WHERE json_extract(document,'$.status')='open' AND json_extract(document,'$.schedule_id') IS NOT NULL ORDER BY updated_at LIMIT 20")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }

    pub fn summaries(&self, limit: u32, offset: u32) -> Result<Vec<serde_json::Value>> {
        anyhow::ensure!((1..=100).contains(&limit), "limit must be 1–100");
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let mut stmt=conn.prepare("SELECT json_object('id',id,'title',json_extract(document,'$.title'),'service',json_extract(document,'$.service'),'status',json_extract(document,'$.status'),'updated_at',updated_at,'evidence_count',json_array_length(document,'$.evidence')) FROM incidents ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2")?;
        let rows = stmt
            .query_map(params![limit, offset], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }
}
