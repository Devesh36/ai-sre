//! Explicit, bounded HTTP operations. A catalog entry is an operation, not a claim of full vendor support.
use crate::{
    config::{check_env, validate_url},
    tools::bounded_json,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub name: String,
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default)]
    pub header_env: BTreeMap<String, String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
}

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

pub fn catalog() -> Vec<Operation> {
    vec![
        op!(
            "kubernetes",
            "pods",
            "List namespace pods",
            "/api/v1/namespaces/{namespace}/pods",
            ["labelSelector", "limit"]
        ),
        op!(
            "kubernetes",
            "events",
            "List namespace events",
            "/api/v1/namespaces/{namespace}/events",
            ["fieldSelector", "limit"]
        ),
        op!(
            "kubernetes",
            "deployments",
            "List namespace deployments",
            "/apis/apps/v1/namespaces/{namespace}/deployments",
            ["labelSelector", "limit"]
        ),
        op!(
            "kubernetes",
            "nodes",
            "List node health and capacity",
            "/api/v1/nodes",
            ["limit"]
        ),
        op!(
            "github",
            "commits",
            "Read recent repository commits",
            "/repos/{owner}/{repository}/commits",
            ["sha", "per_page"]
        ),
        op!(
            "github",
            "deployments",
            "Read repository deployments",
            "/repos/{owner}/{repository}/deployments",
            ["environment", "per_page"]
        ),
        op!(
            "github",
            "runs",
            "Read CI workflow runs",
            "/repos/{owner}/{repository}/actions/runs",
            ["status", "per_page"]
        ),
        op!(
            "github",
            "issues",
            "Read repository issues",
            "/repos/{owner}/{repository}/issues",
            ["state", "per_page"]
        ),
        op!(
            "gitlab",
            "pipelines",
            "Read project pipelines",
            "/api/v4/projects/{project}/pipelines",
            ["status", "per_page"]
        ),
        op!(
            "gitlab",
            "commits",
            "Read project commits",
            "/api/v4/projects/{project}/repository/commits",
            ["ref_name", "per_page"]
        ),
        op!(
            "gitlab",
            "deployments",
            "Read project deployments",
            "/api/v4/projects/{project}/deployments",
            ["per_page"]
        ),
        op!(
            "bitbucket",
            "commits",
            "Read repository commits",
            "/2.0/repositories/{workspace}/{repository}/commits",
            ["pagelen"]
        ),
        op!(
            "bitbucket",
            "pipelines",
            "Read repository pipelines",
            "/2.0/repositories/{workspace}/{repository}/pipelines",
            ["pagelen"]
        ),
        op!(
            "grafana",
            "dashboards",
            "Search dashboards",
            "/api/search",
            ["query", "limit"]
        ),
        op!(
            "grafana",
            "annotations",
            "Read deployment and incident annotations",
            "/api/annotations",
            ["from", "to", "limit"]
        ),
        op!(
            "grafana",
            "health",
            "Read Grafana health",
            "/api/health",
            []
        ),
        op!(
            "tempo",
            "trace",
            "Read a distributed trace",
            "/api/traces/{trace_id}",
            []
        ),
        op!(
            "tempo",
            "search",
            "Search distributed traces",
            "/api/search",
            ["q", "start", "end", "limit"]
        ),
        op!(
            "datadog",
            "monitors",
            "Read monitor states",
            "/api/v1/monitor",
            ["name", "page", "page_size"]
        ),
        op!(
            "datadog",
            "events",
            "Read event timeline",
            "/api/v1/events",
            ["start", "end", "tags"]
        ),
        op!(
            "datadog",
            "metrics",
            "Query metrics",
            "/api/v1/query",
            ["from", "to", "query"]
        ),
        op!(
            "sentry",
            "issues",
            "Read organization issues",
            "/api/0/organizations/{organization}/issues/",
            ["query", "limit"]
        ),
        op!(
            "sentry",
            "events",
            "Read issue events",
            "/api/0/issues/{issue_id}/events/",
            ["full"]
        ),
        op!(
            "sentry",
            "projects",
            "Read organization projects",
            "/api/0/organizations/{organization}/projects/",
            []
        ),
        op!(
            "pagerduty",
            "incidents",
            "Read incidents",
            "/incidents",
            ["since", "until", "limit", "offset"]
        ),
        op!(
            "pagerduty",
            "services",
            "Read service configuration",
            "/services",
            ["limit", "offset"]
        ),
        op!(
            "pagerduty",
            "timeline",
            "Read incident log entries",
            "/incidents/{incident_id}/log_entries",
            ["limit"]
        ),
        op!(
            "opsgenie",
            "alerts",
            "Read alerts",
            "/v2/alerts",
            ["query", "limit", "offset"]
        ),
        op!(
            "opsgenie",
            "incident",
            "Read incident details",
            "/v1/incidents/{incident_id}",
            []
        ),
        op!(
            "jira",
            "issue",
            "Read an issue",
            "/rest/api/3/issue/{issue_key}",
            ["fields"]
        ),
        op!(
            "jira",
            "search",
            "Search issues with JQL",
            "/rest/api/3/search/jql",
            ["jql", "maxResults"]
        ),
        op!(
            "servicenow",
            "incidents",
            "Read incident records",
            "/api/now/table/incident",
            ["sysparm_query", "sysparm_limit"]
        ),
        op!(
            "argocd",
            "applications",
            "Read deployment application status",
            "/api/v1/applications",
            ["selector"]
        ),
        op!(
            "argocd",
            "application",
            "Read application details",
            "/api/v1/applications/{application}",
            []
        ),
        op!(
            "jenkins",
            "builds",
            "Read job build history",
            "/job/{job}/api/json",
            ["tree"]
        ),
        op!(
            "jenkins",
            "build",
            "Read build details",
            "/job/{job}/{build}/api/json",
            []
        ),
        op!(
            "airflow",
            "dags",
            "Read DAG states",
            "/api/v1/dags",
            ["limit", "offset"]
        ),
        op!(
            "airflow",
            "runs",
            "Read DAG runs",
            "/api/v1/dags/{dag}/dagRuns",
            ["limit", "order_by"]
        ),
        op!(
            "rabbitmq",
            "queues",
            "Read queue depth and consumers",
            "/api/queues/{vhost}",
            ["page", "page_size"]
        ),
        op!(
            "rabbitmq",
            "overview",
            "Read broker health",
            "/api/overview",
            []
        ),
        op!(
            "rabbitmq",
            "connections",
            "Read broker connections",
            "/api/connections",
            ["page", "page_size"]
        ),
        op!("honeycomb", "datasets", "Read datasets", "/1/datasets", []),
        op!(
            "honeycomb",
            "triggers",
            "Read dataset triggers",
            "/1/triggers/{dataset}",
            []
        ),
        op!(
            "incident_io",
            "incidents",
            "Read incidents",
            "/v2/incidents",
            ["page_size", "after"]
        ),
        op!(
            "posthog",
            "events",
            "Read project events",
            "/api/projects/{project}/events/",
            ["event", "limit", "after"]
        ),
        op!(
            "posthog",
            "insights",
            "Read saved insights",
            "/api/projects/{project}/insights/",
            ["limit"]
        ),
        op!(
            "betterstack",
            "incidents",
            "Read uptime incidents",
            "/api/v2/incidents",
            ["per_page"]
        ),
        op!(
            "betterstack",
            "monitors",
            "Read uptime monitors",
            "/api/v2/monitors",
            ["per_page"]
        ),
        op!(
            "vercel",
            "deployments",
            "Read deployments",
            "/v6/deployments",
            ["projectId", "teamId", "limit"]
        ),
        op!(
            "vercel",
            "deployment",
            "Read deployment status",
            "/v13/deployments/{deployment}",
            ["teamId"]
        ),
        op!(
            "slack",
            "history",
            "Read channel history",
            "/api/conversations.history",
            ["channel", "oldest", "latest", "limit"]
        ),
        op!(
            "slack",
            "thread",
            "Read incident discussion",
            "/api/conversations.replies",
            ["channel", "ts", "limit"]
        ),
        op!(
            "discord",
            "messages",
            "Read channel messages",
            "/api/v10/channels/{channel}/messages",
            ["limit", "before"]
        ),
        op!(
            "google_docs",
            "document",
            "Read a document",
            "/v1/documents/{document}",
            []
        ),
        op!(
            "supabase",
            "rows",
            "Read a configured table via REST",
            "/rest/v1/{table}",
            ["select", "limit", "order"]
        ),
        op!(
            "elasticsearch",
            "health",
            "Read cluster health",
            "/_cluster/health",
            []
        ),
        op!(
            "opensearch",
            "health",
            "Read cluster health",
            "/_cluster/health",
            []
        ),
        Operation {
            kind: "elasticsearch",
            name: "search",
            description: "Search a configured index",
            path: "/{index}/_search",
            query: &[],
            search_body: true,
        },
        Operation {
            kind: "opensearch",
            name: "search",
            description: "Search a configured index",
            path: "/{index}/_search",
            query: &[],
            search_body: true,
        },
    ]
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 20
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl Connection {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            valid_name(&self.name),
            "connection names must contain 1–20 lowercase letters, digits, or underscores"
        );
        ensure!(
            catalog().iter().any(|o| o.kind == self.kind),
            "unsupported connector kind: {}",
            self.kind
        );
        validate_url(&self.url)?;
        check_env(&self.token_env)?;
        for (header, var) in &self.header_env {
            let parsed = reqwest::header::HeaderName::from_bytes(header.as_bytes())?;
            ensure!(
                !matches!(
                    parsed.as_str(),
                    "host" | "content-length" | "transfer-encoding" | "connection"
                ),
                "transport header cannot be overridden"
            );
            check_env(&Some(var.clone()))?;
        }
        Ok(())
    }

    pub fn definitions(&self) -> Vec<Value> {
        catalog().into_iter().filter(|o| o.kind == self.kind).map(|o| {
            let mut properties = serde_json::Map::new();
            let mut required = vec![];
            for name in placeholders(o.path) {
                if !self.parameters.contains_key(name) { properties.insert(name.into(), json!({"type":"string","minLength":1,"maxLength":1024})); required.push(name); }
            }
            for key in o.query { properties.insert((*key).into(), json!({"type":"string","maxLength":2000})); }
            if o.search_body { properties.insert("body".into(), json!({"type":"object","description":"Read-only search DSL; bounded response required"})); required.push("body"); }
            json!({"type":"function","function":{"name":format!("conn_{}_{}",self.name,o.name),"description":format!("{} ({})",o.description,self.name),"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})
        }).collect()
    }

    pub async fn call(
        &self,
        client: &reqwest::Client,
        operation: &str,
        args: &Value,
    ) -> Result<Value> {
        let op = catalog()
            .into_iter()
            .find(|o| o.kind == self.kind && o.name == operation)
            .ok_or_else(|| anyhow::anyhow!("operation is not allowed"))?;
        let args = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("tool arguments must be an object"))?;
        let path_keys = placeholders(op.path);
        for key in args.keys() {
            if key != "body" {
                ensure!(args[key].is_string(), "argument {key} must be a string");
            }
            ensure!(
                (path_keys.contains(&key.as_str()) && !self.parameters.contains_key(key))
                    || op.query.contains(&key.as_str())
                    || (op.search_body && key == "body"),
                "unexpected or fixed argument: {key}"
            );
        }
        let mut url = reqwest::Url::parse(&self.url)?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("invalid base URL"))?;
            segments.pop_if_empty();
            for segment in op.path.trim_start_matches('/').split('/') {
                if let Some(key) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    let value = self
                        .parameters
                        .get(key)
                        .map(String::as_str)
                        .or_else(|| args.get(key).and_then(Value::as_str))
                        .ok_or_else(|| anyhow::anyhow!("missing path argument: {key}"))?;
                    ensure!(
                        !value.is_empty() && value.len() <= 1024 && value != "." && value != "..",
                        "invalid path argument"
                    );
                    segments.push(value);
                } else {
                    segments.push(segment);
                }
            }
        }
        for key in op.query {
            if let Some(value) = args
                .get(*key)
                .and_then(Value::as_str)
                .or_else(|| self.parameters.get(*key).map(String::as_str))
            {
                ensure!(value.len() <= 2000, "query argument too long");
                url.query_pairs_mut().append_pair(key, value);
            } else if matches!(
                *key,
                "limit" | "per_page" | "page_size" | "pagelen" | "maxResults" | "sysparm_limit"
            ) {
                url.query_pairs_mut().append_pair(key, "50");
            }
        }
        let mut request = if op.search_body {
            let mut body = args
                .get("body")
                .and_then(Value::as_object)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("search body must be an object"))?;
            ensure!(
                !body.contains_key("script_fields") && !body.contains_key("runtime_mappings"),
                "scripted search fields are not allowed"
            );
            body.insert(
                "size".into(),
                json!(
                    body.get("size")
                        .and_then(Value::as_u64)
                        .unwrap_or(50)
                        .min(100)
                ),
            );
            client.post(url).json(&body)
        } else {
            client.get(url)
        };
        request = request
            .header("User-Agent", "ai-sre/0.1")
            .header("Accept", "application/json");
        if let Some(var) = &self.token_env {
            let token = std::env::var(var)?;
            request = match self.kind.as_str() {
                "pagerduty" => request.header("Authorization", format!("Token token={token}")),
                "opsgenie" => request.header("Authorization", format!("GenieKey {token}")),
                "discord" => request.header("Authorization", format!("Bot {token}")),
                "honeycomb" => request.header("X-Honeycomb-Team", token),
                _ => request.bearer_auth(token),
            };
        }
        for (header, var) in &self.header_env {
            request = request.header(header, std::env::var(var)?);
        }
        let response = bounded_json(
            request
                .send()
                .await
                .map_err(|_| crate::harness::TransportFault)?,
        )
        .await?;
        ensure!(
            response.get("ok") != Some(&Value::Bool(false)),
            "connector reported an API error"
        );
        ensure!(
            response.get("error").is_none(),
            "connector reported an error"
        );
        Ok(response)
    }
}

fn placeholders(path: &str) -> Vec<&str> {
    path.split('/')
        .filter_map(|s| s.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
        .collect()
}
