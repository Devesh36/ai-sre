# Local and remote MCP tools

The assistant uses the same tool registry from the CLI, REPL, and web investigations.
Configure a server and an explicit list of tools in your TOML configuration. Only
advertised tools with `annotations.readOnlyHint = true` enter the registry.

For an already installed local server:

```toml
[[mcp]]
name = "observability"
mode = "stdio"
command = "/absolute/path/to/your-mcp-server"
args = ["--read-only"]
allowed_tools = ["inspect"]
timeout_seconds = 30
env = { VENDOR_TOKEN = "OBSERVABILITY_READ_TOKEN" }
```

Replace the executable, arguments, and tool name with those supported by your
server. `env` maps a child environment variable to a parent environment variable;
set `OBSERVABILITY_READ_TOKEN` outside the config file. Omit `env` if unnecessary.
No packages are downloaded or installed automatically. The process starts in the
assistant's working directory. Arguments are passed literally without a shell;
the model cannot change the executable, arguments, or environment mappings.

```sh
ai-sre --config config.toml tool mcp_observability_inspect '{}'
ai-sre --config config.toml
```

An existing remote configuration remains valid:

```toml
[[mcp]]
name = "remote"
mode = "streamable-http" # default when omitted
url = "https://your-server.example/mcp"
token_env = "MCP_READ_TOKEN"
allowed_tools = ["inspect"]
```

Remote mode rejects local-process options; stdio rejects URL/token options.
Both negotiate MCP `2025-03-26`. Discovery accepts at most eight pages and
32 allowlisted tool names. A server's read-only annotation is a trust assertion,
not a sandbox or proof that its tools cannot write.

## Process and protocol bounds

- One child and serialized operations per client; clones share that child.
- Newline-delimited JSON-RPC, at most 256 KiB per local request/response frame.
- At most 64 incoming messages and 1 MiB of incoming data per local exchange.
- A per-RPC deadline of 1–300 seconds (default 30), including write/read work.
  Waiting for the serialized operation lock is outside that deadline; the agent's
  overall run/tool deadlines still apply. Discovery can make up to ten RPCs.
- Local server pings receive a response while a request is pending. Other server
  requests receive `-32601`. Bounded notifications are consumed but not displayed.
- Timeout, invalid protocol data, EOF, or dropped in-flight futures destroy the
  local session. Subsequent calls fail until explicit initialization; tool calls
  are never silently replayed. A tool-level `isError` leaves the session usable.
- Dropping the last client kills its direct child; Tokio arranges reaping.
  Detached descendants are not managed. There is no process-tree sandbox.
- Child stderr goes to the null device, avoiding terminal leakage and blocked
  diagnostic pipes. stdout must contain only protocol messages.

The child inherits a small runtime environment (PATH, HOME, user/temp/platform
variables) plus proxy and CA settings. Other credentials require explicit `env`
mappings, whose values are included in the assistant's redaction list. NO_COLOR
and TERM are fixed to `1` and `dumb`. A trusted local program still has the user's
filesystem permissions and can read credentials from files; environment filtering
is not isolation. Configure only installed programs you intend to run.

Legacy HTTP+SSE, OAuth, resources/prompts, subscriptions, server sampling and
elicitation, and idle background server-request handling are not implemented.
Results larger than the receive limit fail; use a server's paging/summary tools.
Third-party server compatibility and Windows process behavior are unverified.
