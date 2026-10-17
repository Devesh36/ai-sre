use crate::{agent, config::Config, domain::Incident, store::Store, tools::Toolbox};
use anyhow::{Result, ensure};
use uuid::Uuid;
pub async fn ask(
    config: &Config,
    store: &Store,
    question: &str,
    service: &str,
    id: Option<Uuid>,
) -> Result<Incident> {
    ensure!(
        !question.trim().is_empty() && question.len() <= 4000,
        "question must contain 1–4000 bytes"
    );
    let tools = Toolbox::new(config.clone())?;
    let question = tools.redact(question);
    let id = match id {
        Some(id) => id,
        None => {
            store
                .insert(
                    &Incident::new(question.clone(), tools.redact(service))?,
                    None,
                )?
                .0
                .id
        }
    };
    let incident = store.claim(id, Some(&question))?;
    let run = agent::investigate(config, store, incident);
    tokio::pin!(run);
    tokio::select! {result=&mut run=>result,_=tokio::signal::ctrl_c()=>{store.cancel(id)?;run.await}}
}
