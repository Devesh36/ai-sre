# Deployment

## Build and start

```sh
cargo build --release --locked
./target/release/ai-sre --config sre.toml serve
```

The executable embeds all web assets and uses bundled SQLite. It needs a writable database directory and system CA certificates for outbound TLS. No Node, Python, database server, or hosted account is required for offline operation. Builds are pinned by `rust-toolchain.toml` and `Cargo.lock`.

For remote access, set a randomly generated `SRE_API_TOKEN` of at least 24 bytes in the service environment and bind explicitly:

```sh
./target/release/ai-sre --config /etc/ai-sre/sre.toml serve --bind 0.0.0.0:8080
```

Place it behind a TLS reverse proxy and restrict network access to trusted operators. Supply the same token through the dashboard authentication dialog. Tokens remain in browser memory and are lost on reload. Do not expose an unauthenticated loopback server through a tunnel or reverse proxy.

Run as a dedicated non-root account. Make the database directory private to that account. Store tokens in your service manager or secret manager, not in tracked files. The application does not provide per-user roles or tenant isolation.

## Process lifecycle

Run under your service manager with a persistent working directory. SIGINT and SIGTERM stop intake and allow active investigations to finish; permit at least 310 seconds for shutdown. Hard termination requires recovery of affected incidents:

```sh
ai-sre --config /etc/ai-sre/sre.toml recover INCIDENT_UUID --worker-stopped
ai-sre --config /etc/ai-sre/sre.toml ask 'Resume after interruption' --incident INCIDENT_UUID
```

Only recover after confirming the old worker process is gone. Never run recovery concurrently with that worker.

The server runs the persisted schedule queue while `serve` is active. Tool checkpoints preserve collected evidence, but recovery starts a new investigation run; it does not resume an interrupted HTTP request. Run one scheduler process per database.

Configured action templates can change infrastructure after explicit review. The reviewer name is an audit label, not verified identity; all holders of the API token have the same authority. If execution is interrupted or its outcome is unknown, reconcile the target state manually before proposing another action. The executor never retries mutations or automatically rolls them back. See [operational workflows](operations.md) for configuration and approval expiry.

Use SQLite’s backup API or stop the service before copying the database. Copying only the `.db` file while WAL writes are active can omit recent data. Database contents are not encrypted by the application.

## Cloud development environment

Run `bash scripts/setup-cloud.sh` from the repository. It installs Rust 1.90.0 under `/workspace/.toolchains`, uses verified HTTPS and standard Rust artifact verification, and builds/tests using the lockfile. It leaves credentials and source files untouched.

For subsequent shells:

```sh
source scripts/cloud-env.sh
cargo run --locked -- demo
cargo run --locked -- --config examples/demo.toml serve
```

Live processes do not survive environment snapshots. Restart the server for each task and check `/healthz` plus the incident API. The demo needs no external service credentials. Live production integrations and hosted model calls need operator configuration and access to the chosen destinations.
