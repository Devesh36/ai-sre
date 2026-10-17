use serde::Serialize;

pub use connection::{Connection, valid_name};
#[derive(Clone, Serialize)]
pub struct Operation {
    pub kind: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub path: &'static str,
    pub query: &'static [&'static str],
    pub search_body: bool,
}

macro_rules! op {
    ($kind:literal,$name:literal,$desc:literal,$path:literal,[$($q:literal),*]) => {
        Operation { kind:$kind,name:$name,description:$desc,path:$path,query:&[$($q),*],search_body:false }
    };
}

pub mod airflow;
pub mod argocd;
pub mod betterstack;
pub mod bitbucket;
pub mod connection;
pub mod datadog;
pub mod discord;
pub mod elasticsearch;
pub mod github;
pub mod gitlab;
pub mod google_docs;
pub mod grafana;
pub mod honeycomb;
pub mod incident_io;
pub mod jenkins;
pub mod jira;
pub mod kubernetes;
pub mod mcp;
pub mod opensearch;
pub mod opsgenie;
pub mod pagerduty;
pub mod posthog;
pub mod rabbitmq;
pub mod sentry;
pub mod servicenow;
pub mod slack;
pub mod supabase;
pub mod tempo;
pub mod vercel;

pub fn catalog() -> Vec<Operation> {
    let mut operations = Vec::new();
    operations.extend(kubernetes::operations());
    operations.extend(github::operations());
    operations.extend(gitlab::operations());
    operations.extend(bitbucket::operations());
    operations.extend(grafana::operations());
    operations.extend(tempo::operations());
    operations.extend(datadog::operations());
    operations.extend(sentry::operations());
    operations.extend(pagerduty::operations());
    operations.extend(opsgenie::operations());
    operations.extend(jira::operations());
    operations.extend(servicenow::operations());
    operations.extend(argocd::operations());
    operations.extend(jenkins::operations());
    operations.extend(airflow::operations());
    operations.extend(rabbitmq::operations());
    operations.extend(honeycomb::operations());
    operations.extend(incident_io::operations());
    operations.extend(posthog::operations());
    operations.extend(betterstack::operations());
    operations.extend(vercel::operations());
    operations.extend(slack::operations());
    operations.extend(discord::operations());
    operations.extend(google_docs::operations());
    operations.extend(supabase::operations());
    operations.extend(elasticsearch::operations());
    operations.extend(opensearch::operations());
    operations
}
