# vps-monitor

> Lightweight, secure, read-only VPS monitoring WebUI written in Rust.
> [中文文件 / Chinese](./README.zh.md)

A single-binary self-hosted dashboard that lets you peek at your VPS — CPU, memory, disks, network, processes, PM2 apps, Docker containers and systemd services — and tail their logs in real time, all over an SSH tunnel. Designed to add as little attack surface as humanly possible.

```text
[ Your laptop ] --ssh -L 8443:localhost:8443--> [ VPS:127.0.0.1:8443 ]
                                                       |
                                                       +-- axum HTTP / WebSocket server
                                                       +-- sysinfo (CPU/mem/disk/net)
                                                       +-- bollard (docker.sock)
                                                       +-- pm2 jlist + log file tailing
                                                       +-- journalctl streaming
                                                       +-- SQLite (users / sessions / audit)
```

## Why another monitoring tool?

Netdata, Grafana, Glances, Dozzle — they're all great, but most either expose a public port, ship a heavy frontend, or focus on time-series storage. This is the opposite:

- **Read-only by design.** No shell, no `kill`, no restart. Eliminates a whole category of compromise scenarios.
- **Loopback-only.** Hard-fails at startup if you try to bind to a public address. Reachable only via SSH tunnel (or Tailscale, etc.).
- **Single static binary, ~12 MB.** No Node, no Python, no PHP, no docker-compose. Frontend is embedded with `rust-embed`.
- **Tiny footprint.** ~30 MB RSS at idle, ~50 MB while streaming a log. Capped by `MemoryMax=200M` in the systemd unit.
- **No external dependencies at runtime** beyond what you'd already have: `pm2`, `docker`, `systemctl`, `journalctl`.

## Features

| Tab | What you get |
| --- | --- |
| Overview | CPU, memory, swap, network throughput, load average, disks. Live SSE-pushed every 2 s. |
| Processes | Top-N processes by CPU or RAM. |
| PM2 | Apps with status, CPU, memory, restart count. Click to tail stdout/stderr. |
| Docker | Containers via local docker socket. Click to tail logs (stdout + stderr). |
| Services | systemd units. Click to tail journalctl in real time. |
| Audit | Every login + log view recorded with user, IP and target. |

## Security model

Threat model: someone gains the ability to make HTTP requests to the dashboard, OR the running binary itself is compromised.

| Mitigation | What it does |
| --- | --- |
| Bind to loopback / private IP | Public-IP binds are rejected at startup with a hard failure. |
| SSH tunnel access | `ssh -L 8443:localhost:8443 user@vps`. The SSH layer handles encryption, key auth, and port hiding. |
| Argon2id password hashing | OWASP-recommended params (m = 19 MiB, t = 2, p = 1). |
| Failed-login lockout | 5 failures → 15 min freeze. Configurable. |
| HttpOnly + SameSite=Strict cookies | Mitigates XSS token exfiltration and CSRF on cross-origin POSTs. |
| `X-Requested-With: fetch` requirement | All mutation endpoints reject non-XHR requests. |
| Sliding sessions | Cookie extends on use, dies after 7 days inactive. |
| No process control | No `POST` endpoints exist that touch the system — only login/logout. |
| Strict allow-list validation | Unit names, container IDs, PM2 names checked against regex before being passed to subprocesses. No shell, no string concatenation. |
| systemd sandbox | `ProtectSystem=strict`, `NoNewPrivileges`, `SystemCallFilter=@system-service`, etc. |
| Audit log | login_ok / login_fail / login_locked / logout / view_logs (with target) — retained 90 days. |

> [!IMPORTANT]
> Even with all of this, never expose this dashboard to the public internet. The SSH-tunnel pattern is a deliberate constraint, not a default. If you put it behind a public domain, the threat model changes — you should add at minimum: 2FA, mTLS, fail2ban, WAF.

## Build

Needs Rust ≥ 1.75. On Debian/Ubuntu:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt install -y build-essential pkg-config
cargo build --release
```

Result: `target/release/vps-monitor` (~12 MB, stripped).

## Install

Clone the repo on your VPS, then:

```bash
sudo bash scripts/install.sh
```

The installer:

1. Builds the release binary.
2. Creates a `vps-monitor` system user, adds it to the `docker` and `systemd-journal` groups.
3. Installs the binary to `/usr/local/bin/vps-monitor`.
4. Installs the systemd unit to `/etc/systemd/system/vps-monitor.service`.
5. Applies DB migrations to `/var/lib/vps-monitor/db.sqlite`.
6. Prompts you for an initial admin password.
7. Enables + starts the service.

## Usage

From your laptop:

```bash
ssh -L 8443:localhost:8443 user@your-vps
```

Open <http://localhost:8443/login.html>, log in, browse.

## Configuration

`/etc/vps-monitor/config.toml`:

```toml
bind = "127.0.0.1:8443"          # loopback or private IP only
db_path = "/var/lib/vps-monitor/db.sqlite"
session_ttl_secs = 604800        # 7 days, sliding window
audit_retention_days = 90
max_failed_logins = 5
lockout_minutes = 15
# docker_socket = "/var/run/docker.sock"   # override if non-standard
```

## CLI

```text
vps-monitor serve [--config /etc/vps-monitor/config.toml]
vps-monitor migrate [--db /var/lib/vps-monitor/db.sqlite]
vps-monitor user-add <username> [--db ...]
vps-monitor user-passwd <username> [--db ...]
vps-monitor user-list [--db ...]
```

## API

| Method | Path | Description |
| --- | --- | --- |
| POST | `/api/auth/login` | username / password → session cookie |
| POST | `/api/auth/logout` | clear session |
| GET | `/api/auth/me` | current user |
| GET | `/api/metrics` | one-shot metrics snapshot |
| GET | `/api/metrics/stream` | SSE stream, 1 event / 2 s |
| GET | `/api/processes?sort=cpu&limit=20` | top-N processes |
| GET | `/api/pm2` | PM2 apps |
| GET | `/api/docker` | Docker containers |
| GET | `/api/services?state=active` | systemd units |
| WS | `/api/logs/pm2/:name` | tail PM2 stdout/err |
| WS | `/api/logs/docker/:id` | tail container logs |
| WS | `/api/logs/journal/:unit` | `journalctl -fu` |
| GET | `/api/audit?limit=100` | audit entries |

All endpoints (except `/api/auth/login`) require a valid session.

## Project layout

```text
vps-monitor/
├── Cargo.toml
├── README.md / README.zh.md
├── config.toml.example
├── migrations/001_init.sql
├── scripts/install.sh
├── systemd/vps-monitor.service
├── src/
│   ├── main.rs               CLI entry
│   ├── config.rs             config loader + bind validation
│   ├── state.rs              shared state, background tasks
│   ├── error.rs              AppError + IntoResponse
│   ├── auth/                 password hashing, sessions, middleware
│   ├── collectors/           sysinfo, pm2, docker, systemd
│   ├── handlers/             REST + SSE + WebSocket handlers
│   └── db/                   rusqlite + r2d2 pool, migrations, queries
└── static/                   index.html / login.html / app.js / style.css (embedded)
```

## Roadmap (maybe)

- Multiple roles (admin / viewer)
- Optional alerting webhook on threshold breach
- Multi-VPS aggregation (agent + central server)
- 2FA / TOTP

PRs welcome — but please keep the **read-only, single-binary, loopback-only** invariants intact.

## License

MIT. See [LICENSE](LICENSE).
