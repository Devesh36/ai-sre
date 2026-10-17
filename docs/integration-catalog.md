# Native connector operations

The catalog below is generated from the Rust operation registry. Each row is an implemented request template, not complete vendor coverage. Route construction, fixed-scope parameters, bounded responses, and search limits are exercised against local HTTP test servers. Live vendor accounts have not been validated.

Prometheus instant queries and Loki one-hour log queries are separate built-in tools. MCP can supply explicitly allowlisted tools from trusted servers; that does not make an unsupported native integration implemented.

Run `ai-sre catalog` to inspect the registry, or `ai-sre tool TOOL_NAME '{"argument":"value"}' --service SERVICE` to exercise a configured operation. Tool names are `conn_CONNECTION_OPERATION`.

| Connector | Operation | Request | Query arguments |
| --- | --- | --- | --- |
| airflow | dags | GET `/api/v1/dags` | limit, offset |
| airflow | runs | GET `/api/v1/dags/{dag}/dagRuns` | limit, order_by |
| argocd | applications | GET `/api/v1/applications` | selector |
| argocd | application | GET `/api/v1/applications/{application}` | — |
| betterstack | incidents | GET `/api/v2/incidents` | per_page |
| betterstack | monitors | GET `/api/v2/monitors` | per_page |
| bitbucket | commits | GET `/2.0/repositories/{workspace}/{repository}/commits` | pagelen |
| bitbucket | pipelines | GET `/2.0/repositories/{workspace}/{repository}/pipelines` | pagelen |
| datadog | monitors | GET `/api/v1/monitor` | name, page, page_size |
| datadog | events | GET `/api/v1/events` | start, end, tags |
| datadog | metrics | GET `/api/v1/query` | from, to, query |
| discord | messages | GET `/api/v10/channels/{channel}/messages` | limit, before |
| elasticsearch | health | GET `/_cluster/health` | — |
| elasticsearch | search | POST `/{index}/_search` | JSON search body |
| github | commits | GET `/repos/{owner}/{repository}/commits` | sha, per_page |
| github | deployments | GET `/repos/{owner}/{repository}/deployments` | environment, per_page |
| github | runs | GET `/repos/{owner}/{repository}/actions/runs` | status, per_page |
| github | issues | GET `/repos/{owner}/{repository}/issues` | state, per_page |
| gitlab | pipelines | GET `/api/v4/projects/{project}/pipelines` | status, per_page |
| gitlab | commits | GET `/api/v4/projects/{project}/repository/commits` | ref_name, per_page |
| gitlab | deployments | GET `/api/v4/projects/{project}/deployments` | per_page |
| google_docs | document | GET `/v1/documents/{document}` | — |
| grafana | dashboards | GET `/api/search` | query, limit |
| grafana | annotations | GET `/api/annotations` | from, to, limit |
| grafana | health | GET `/api/health` | — |
| honeycomb | datasets | GET `/1/datasets` | — |
| honeycomb | triggers | GET `/1/triggers/{dataset}` | — |
| incident_io | incidents | GET `/v2/incidents` | page_size, after |
| jenkins | builds | GET `/job/{job}/api/json` | tree |
| jenkins | build | GET `/job/{job}/{build}/api/json` | — |
| jira | issue | GET `/rest/api/3/issue/{issue_key}` | fields |
| jira | search | GET `/rest/api/3/search/jql` | jql, maxResults |
| kubernetes | pods | GET `/api/v1/namespaces/{namespace}/pods` | labelSelector, limit |
| kubernetes | events | GET `/api/v1/namespaces/{namespace}/events` | fieldSelector, limit |
| kubernetes | deployments | GET `/apis/apps/v1/namespaces/{namespace}/deployments` | labelSelector, limit |
| kubernetes | nodes | GET `/api/v1/nodes` | limit |
| opensearch | health | GET `/_cluster/health` | — |
| opensearch | search | POST `/{index}/_search` | JSON search body |
| opsgenie | alerts | GET `/v2/alerts` | query, limit, offset |
| opsgenie | incident | GET `/v1/incidents/{incident_id}` | — |
| pagerduty | incidents | GET `/incidents` | since, until, limit, offset |
| pagerduty | services | GET `/services` | limit, offset |
| pagerduty | timeline | GET `/incidents/{incident_id}/log_entries` | limit |
| posthog | events | GET `/api/projects/{project}/events/` | event, limit, after |
| posthog | insights | GET `/api/projects/{project}/insights/` | limit |
| rabbitmq | queues | GET `/api/queues/{vhost}` | page, page_size |
| rabbitmq | overview | GET `/api/overview` | — |
| rabbitmq | connections | GET `/api/connections` | page, page_size |
| sentry | issues | GET `/api/0/organizations/{organization}/issues/` | query, limit |
| sentry | events | GET `/api/0/issues/{issue_id}/events/` | full |
| sentry | projects | GET `/api/0/organizations/{organization}/projects/` | — |
| servicenow | incidents | GET `/api/now/table/incident` | sysparm_query, sysparm_limit |
| slack | history | GET `/api/conversations.history` | channel, oldest, latest, limit |
| slack | thread | GET `/api/conversations.replies` | channel, ts, limit |
| supabase | rows | GET `/rest/v1/{table}` | select, limit, order |
| tempo | trace | GET `/api/traces/{trace_id}` | — |
| tempo | search | GET `/api/search` | q, start, end, limit |
| vercel | deployments | GET `/v6/deployments` | projectId, teamId, limit |
| vercel | deployment | GET `/v13/deployments/{deployment}` | teamId |

## Configuration

```toml
[[connections]]
name = "production"
kind = "kubernetes"
url = "https://cluster.example.com"
token_env = "KUBE_READ_TOKEN"
parameters = { namespace = "production" }

[[connections]]
name = "releases"
kind = "github"
url = "https://api.github.com"
token_env = "REPOSITORY_READ_TOKEN"
parameters = { owner = "your-organization", repository = "your-service" }
```

Configured path parameters cannot be overridden by a model. Values supplied in path segments are URL-encoded. Model arguments cannot select a destination, URL, HTTP method, or credential. Query arguments are strings; bounded list defaults are added when omitted. Catalog operations fetch one page only. Use explicit pagination arguments where available.

For non-bearer credentials, use `header_env = { "HEADER-NAME" = "ENV_VARIABLE" }`. Datadog normally requires `DD-API-KEY` and `DD-APPLICATION-KEY`; GitLab can use `PRIVATE-TOKEN`; RabbitMQ/Jenkins can use an environment-held Authorization header. The code applies provider-specific token schemes for PagerDuty, Opsgenie, Discord, and Honeycomb. Use read-scoped service credentials.

Empty results and provider-specific JSON are retained as evidence; they do not independently establish health. Large responses fail at 256 KiB. Search operations cap returned hits at 100. This does not replace server-side query resource limits. Remote endpoints require HTTPS and redirects are rejected.

Native catalog operations run when a model selects them or an operator invokes `tool`; offline initial collection automatically queries only configured local context, Prometheus, and Loki.
