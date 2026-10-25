use super::Store;
use crate::domain::{
    Incident,
    outcome::{self, Error, History, Input, Outcome},
};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

impl Store {
    /// One transaction checks run ownership, idempotency, correction conflicts, and the history cap.
    /// Outcome writes never update an incident's report, evidence, status, or model context.
    pub fn record_outcome(&self, incident_id: Uuid, mut input: Input) -> Result<(Outcome, bool)> {
        input.normalize()?;
        let mut conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT document FROM outcomes WHERE id=?1",
                [input.id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(doc) = existing {
            let saved: Outcome = serde_json::from_str(&doc)?;
            ensure!(
                saved.same_request(incident_id, &input),
                Error::Conflict("record ID already exists with different content")
            );
            return Ok((saved, false));
        }
        let doc: Option<String> = tx
            .query_row(
                "SELECT document FROM incidents WHERE id=?1",
                [incident_id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let incident: Incident = serde_json::from_str(&doc.ok_or(Error::NotFound)?)?;
        if let Some(previous) = input.supersedes {
            let owner: Option<String> = tx
                .query_row(
                    "SELECT incident_id FROM outcomes WHERE id=?1",
                    [previous.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            ensure!(
                owner.as_deref() == Some(incident_id.to_string().as_str()),
                Error::Invalid("superseded assessment must belong to this incident")
            );
            let corrected: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM outcomes WHERE supersedes=?1)",
                [previous.to_string()],
                |r| r.get(0),
            )?;
            ensure!(
                !corrected,
                Error::Conflict("assessment already has a correction; reload its history")
            );
        }
        let count: usize = tx.query_row(
            "SELECT count(*) FROM outcomes WHERE incident_id=?1",
            [incident_id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(
            count < outcome::LIMIT,
            Error::Conflict("incident has reached its 100-assessment history limit")
        );
        let record = outcome::create(&incident, input)?;
        tx.execute(
            "INSERT INTO outcomes(id,incident_id,supersedes,document) VALUES(?1,?2,?3,?4)",
            params![
                record.id.to_string(),
                incident_id.to_string(),
                record.supersedes.map(|id| id.to_string()),
                serde_json::to_string(&record)?
            ],
        )?;
        tx.commit()?;
        Ok((record, true))
    }

    pub fn outcomes(&self, incident_id: Uuid) -> Result<History> {
        let conn = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM incidents WHERE id=?1)",
            [incident_id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(exists, Error::NotFound);
        let mut stmt = conn.prepare(
            "SELECT document FROM outcomes WHERE incident_id=?1 ORDER BY rowid DESC LIMIT ?2",
        )?;
        let docs = stmt
            .query_map(
                params![incident_id.to_string(), outcome::LIMIT as u32],
                |r| r.get::<_, String>(0),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(History {
            incident_id,
            limit: outcome::LIMIT,
            records: docs
                .into_iter()
                .map(|s| Ok(serde_json::from_str(&s)?))
                .collect::<Result<Vec<Outcome>>>()?,
            notice: outcome::NOTICE,
        })
    }
}
