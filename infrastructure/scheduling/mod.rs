use crate::domain::Incident;
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;
#[derive(Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: Uuid,
    pub expression: String,
    pub timezone: String,
    pub question: String,
    pub service: String,
    pub enabled: bool,
    pub next_at: DateTime<Utc>,
}
impl Schedule {
    pub fn new(
        expression: String,
        timezone: String,
        question: String,
        service: String,
    ) -> Result<Self> {
        Incident::new(question.clone(), service.clone())?;
        let next_at = next_occurrence(&expression, &timezone, Utc::now())?;
        Ok(Self {
            id: Uuid::new_v4(),
            expression,
            timezone,
            question,
            service,
            enabled: true,
            next_at,
        })
    }
}
pub fn next_occurrence(
    expression: &str,
    timezone: &str,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    let schedule = cron::Schedule::from_str(expression)?;
    let zone = chrono_tz::Tz::from_str(timezone)?;
    let next = schedule
        .after(&after.with_timezone(&zone))
        .next()
        .ok_or_else(|| anyhow::anyhow!("cron has no future occurrence"))?
        .with_timezone(&Utc);
    ensure!(
        (next - after).num_seconds() >= 1,
        "schedule must advance time"
    );
    Ok(next)
}

pub async fn scheduler(app: crate::api::App, mut stop: tokio::sync::watch::Receiver<bool>) {
    loop {
        if *stop.borrow() {
            break;
        }
        if let Err(e) = app.store.enqueue_due(chrono::Utc::now()) {
            eprintln!("Scheduler enqueue failed: {e}");
        }
        if let Ok(queued) = app.store.queued() {
            for incident in queued {
                let Ok(permit) = app.capacity.clone().try_acquire_owned() else {
                    break;
                };
                let Ok(claimed) = app.store.claim(incident.id, None) else {
                    continue;
                };
                let worker = app.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Err(e) =
                        crate::agent::investigate(&worker.config, &worker.store, claimed).await
                    {
                        eprintln!("Scheduled investigation failed: {e}");
                    }
                });
            }
        }
        tokio::select! {_=tokio::time::sleep(std::time::Duration::from_secs(1))=>{},result=stop.changed()=>{if result.is_err() || *stop.borrow(){break;}}}
    }
}
