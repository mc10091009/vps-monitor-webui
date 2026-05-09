# vps-monitor

> 用 Rust 寫的輕量、安全、唯讀 VPS 監控 WebUI。
> [English](./README.md)

單一可執行檔的自架儀表板,讓你查看 VPS 的 CPU、記憶體、磁碟、網路、進程、PM2 應用、Docker 容器、systemd 服務,並且即時 tail 它們的日誌 — 全部走 SSH Tunnel,不對外暴露。設計目標是「攻擊面盡可能小」。

```text
[ 你的筆電 ] --ssh -L 8443:localhost:8443--> [ VPS:127.0.0.1:8443 ]
                                                       |
                                                       +-- axum HTTP / WebSocket server
                                                       +-- sysinfo (CPU/mem/disk/net)
                                                       +-- bollard (docker.sock)
                                                       +-- pm2 jlist + 日誌檔 tail
                                                       +-- journalctl 串流
                                                       +-- SQLite (用戶 / session / 審計)
```

## 為什麼又一個監控工具?

Netdata、Grafana、Glances、Dozzle 都很好,但大多數要嘛開公網 port、要嘛前端肥、要嘛專注於時序儲存。這個專案剛好相反:

- **設計上唯讀。** 沒有 shell、沒有 `kill`、沒有重啟。整類攻擊情境直接消除。
- **只綁 loopback。** 啟動時校驗 bind 位址,公網 IP 一律 hard-fail。只能透過 SSH Tunnel (或 Tailscale 等) 訪問。
- **單一靜態 binary,~12 MB。** 沒 Node、沒 Python、沒 PHP、沒 docker-compose。前端用 `rust-embed` 直接嵌進 binary。
- **占用低。** 空閒 ~30 MB RSS,串流 1 個日誌時 ~50 MB。systemd unit 用 `MemoryMax=200M` 加上限。
- **runtime 不依賴額外東西**:你 VPS 本來就有的 `pm2`、`docker`、`systemctl`、`journalctl`。

## 功能

| 分頁 | 內容 |
| --- | --- |
| Overview | CPU、記憶體、swap、網路吞吐、load average、磁碟。SSE 每 2 秒推一次。 |
| Processes | 按 CPU 或 RAM 排序的 top-N 進程。 |
| PM2 | PM2 應用列表 (狀態/CPU/記憶體/重啟次數)。點任一個 tail stdout/stderr。 |
| Docker | 透過本機 docker socket 看容器。點任一個 tail logs (stdout + stderr)。 |
| Services | systemd unit 列表。點任一個即時 tail journalctl。 |
| Audit | 每次登入、查看日誌都記錄 (用戶、IP、目標)。 |

## 安全模型

威脅模型:有人能對 dashboard 發 HTTP 請求,**或** binary 本身被攻陷。

| 防護 | 作用 |
| --- | --- |
| 只綁 loopback / 私網 IP | 啟動時校驗,公網 IP 直接 hard-fail。 |
| SSH Tunnel 訪問 | `ssh -L 8443:localhost:8443 user@vps`。SSH 層處理加密、金鑰認證、port 隱藏。 |
| Argon2id 密碼雜湊 | OWASP 推薦參數 (m = 19 MiB, t = 2, p = 1)。 |
| 失敗鎖定 | 連續 5 次失敗鎖 15 分鐘,可調。 |
| HttpOnly + SameSite=Strict cookie | 阻擋 XSS 偷 token 與跨站 CSRF。 |
| 必須帶 `X-Requested-With: fetch` header | 所有 mutation endpoint 拒絕非 XHR 請求。 |
| Sliding session | cookie 使用時延期,7 天無活動失效。 |
| 不允許進程控制 | 沒有任何 POST endpoint 能動到系統 — 只有登入/登出。 |
| 嚴格 allow-list 校驗 | unit 名、容器 ID、PM2 名 全過 regex 才能進子進程。不拼字串、不走 shell。 |
| systemd 沙盒 | `ProtectSystem=strict`、`NoNewPrivileges`、`SystemCallFilter=@system-service` 等。 |
| 審計 log | login_ok / login_fail / login_locked / logout / view_logs (含目標),保留 90 天。 |

> [!IMPORTANT]
> 即使有以上所有防護,**永遠不要**把這個 dashboard 直接暴露到公網。SSH Tunnel 是刻意的設計約束,不是預設配置。如果你硬要放在公網域名後面,威脅模型就變了 — 至少加上 2FA、mTLS、fail2ban、WAF。

## 編譯

需要 Rust ≥ 1.75。在 Debian/Ubuntu:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt install -y build-essential pkg-config
cargo build --release
```

產物:`target/release/vps-monitor` (~12 MB,已 strip)。

## 安裝

在 VPS 上 clone 此 repo,然後:

```bash
sudo bash scripts/install.sh
```

安裝腳本會:

1. 編譯 release binary
2. 建 `vps-monitor` 系統用戶,加入 `docker` 與 `systemd-journal` group
3. 安裝 binary 到 `/usr/local/bin/vps-monitor`
4. 安裝 systemd unit 到 `/etc/systemd/system/vps-monitor.service`
5. 對 `/var/lib/vps-monitor/db.sqlite` 跑 migration
6. 互動式建立第一個 admin
7. enable + start 服務

## 使用

從你的筆電:

```bash
ssh -L 8443:localhost:8443 user@your-vps
```

開瀏覽器 <http://localhost:8443/login.html>,登入,即可瀏覽。

## 設定

`/etc/vps-monitor/config.toml`:

```toml
bind = "127.0.0.1:8443"          # 只允許 loopback / 私網 IP
db_path = "/var/lib/vps-monitor/db.sqlite"
session_ttl_secs = 604800        # 7 天 sliding window
audit_retention_days = 90
max_failed_logins = 5
lockout_minutes = 15
# docker_socket = "/var/run/docker.sock"   # 非標準路徑時自訂
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

| Method | Path | 說明 |
| --- | --- | --- |
| POST | `/api/auth/login` | 用戶名/密碼 → 設 session cookie |
| POST | `/api/auth/logout` | 清除 session |
| GET | `/api/auth/me` | 取得目前用戶 |
| GET | `/api/metrics` | 一次性指標快照 |
| GET | `/api/metrics/stream` | SSE 串流,每 2 秒一筆 |
| GET | `/api/processes?sort=cpu&limit=20` | top-N 進程 |
| GET | `/api/pm2` | PM2 應用 |
| GET | `/api/docker` | Docker 容器 |
| GET | `/api/services?state=active` | systemd units |
| WS | `/api/logs/pm2/:name` | tail PM2 stdout/err |
| WS | `/api/logs/docker/:id` | tail 容器日誌 |
| WS | `/api/logs/journal/:unit` | `journalctl -fu` |
| GET | `/api/audit?limit=100` | 審計記錄 |

除了 `/api/auth/login` 以外,所有 endpoint 都需要有效 session。

## 專案結構

```text
vps-monitor/
├── Cargo.toml
├── README.md / README.zh.md
├── config.toml.example
├── migrations/001_init.sql
├── scripts/install.sh
├── systemd/vps-monitor.service
├── src/
│   ├── main.rs               CLI 入口
│   ├── config.rs             設定載入 + bind 校驗
│   ├── state.rs              共享狀態 / 後台 task
│   ├── error.rs              AppError + IntoResponse
│   ├── auth/                 密碼 hash、session、middleware
│   ├── collectors/           sysinfo、pm2、docker、systemd
│   ├── handlers/             REST + SSE + WebSocket handler
│   └── db/                   rusqlite + r2d2 pool、migration、查詢
└── static/                   index.html / login.html / app.js / style.css (嵌入 binary)
```

## Roadmap

- 多角色 (admin / viewer)
- 閾值告警 webhook
- 多台 VPS 集中 (agent + central server)
- 2FA / TOTP

歡迎 PR — 但請維持 **唯讀、單一 binary、只綁 loopback** 三個底線。

## License

MIT。詳見 [LICENSE](LICENSE)。
