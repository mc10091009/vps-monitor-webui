#!/usr/bin/env bash
# vps-monitor installer — run on the VPS as root (or via sudo).
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
  echo "must run as root: try: sudo $0" >&2
  exit 1
fi

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DST="/usr/local/bin/vps-monitor"
CFG_DIR="/etc/vps-monitor"
DATA_DIR="/var/lib/vps-monitor"
USER_NAME="vps-monitor"

# 1. build dependencies (rusqlite bundled needs a C toolchain)
if ! command -v cc >/dev/null && ! command -v gcc >/dev/null; then
  echo ">> C compiler not found — installing build essentials…"
  if   command -v apt-get >/dev/null; then apt-get update && apt-get install -y build-essential pkg-config curl
  elif command -v dnf     >/dev/null; then dnf install  -y gcc make pkgconfig curl
  elif command -v yum     >/dev/null; then yum install  -y gcc make pkgconfig curl
  elif command -v apk     >/dev/null; then apk add --no-cache build-base pkgconfig curl
  else
    echo "could not find a known package manager (apt/dnf/yum/apk). install gcc, make, pkg-config, curl manually." >&2
    exit 1
  fi
fi

# 2. cargo build
if ! command -v cargo >/dev/null; then
  # When invoked via `sudo` cargo may live in the calling user's $HOME.
  for candidate in \
      "${SUDO_USER:+/home/$SUDO_USER/.cargo/bin}" \
      "/root/.cargo/bin" \
      "$HOME/.cargo/bin"; do
    if [[ -n "$candidate" && -x "$candidate/cargo" ]]; then
      export PATH="$candidate:$PATH"
      echo ">> using cargo from $candidate"
      break
    fi
  done
fi

if ! command -v cargo >/dev/null; then
  echo ">> rustup not found — installing now (non-interactive)…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
  # shellcheck source=/dev/null
  . "$HOME/.cargo/env"
fi

if ! command -v cargo >/dev/null; then
  echo "cargo still not found after rustup install. Please open a new shell and re-run." >&2
  exit 1
fi

echo ">> building release binary (this can take a few minutes on first run)…"
( cd "$REPO_DIR" && cargo build --release )

# 3. user / group
if ! id -u "$USER_NAME" >/dev/null 2>&1; then
  echo ">> creating user '$USER_NAME'"
  useradd --system --shell /usr/sbin/nologin --home-dir /nonexistent "$USER_NAME"
fi

# Add to docker group if docker is installed (so we can read docker.sock).
if getent group docker >/dev/null 2>&1; then
  usermod -aG docker "$USER_NAME"
fi
# systemd-journal group lets us read other users' journals.
if getent group systemd-journal >/dev/null 2>&1; then
  usermod -aG systemd-journal "$USER_NAME"
fi

# 4. install files
echo ">> installing binary -> $BIN_DST"
install -m 755 "$REPO_DIR/target/release/vps-monitor" "$BIN_DST"

mkdir -p "$CFG_DIR"
if [[ ! -f "$CFG_DIR/config.toml" ]]; then
  install -m 640 "$REPO_DIR/config.toml.example" "$CFG_DIR/config.toml"
  chown root:"$USER_NAME" "$CFG_DIR/config.toml"
fi

mkdir -p "$DATA_DIR"
chown -R "$USER_NAME":"$USER_NAME" "$DATA_DIR"
chmod 750 "$DATA_DIR"

echo ">> installing systemd unit"
install -m 644 "$REPO_DIR/systemd/vps-monitor.service" /etc/systemd/system/

# 5. migrate + create initial admin if no users
echo ">> applying migrations"
sudo -u "$USER_NAME" "$BIN_DST" migrate --db "$DATA_DIR/db.sqlite"

if [[ "$(sudo -u "$USER_NAME" "$BIN_DST" user-list --db "$DATA_DIR/db.sqlite" | grep -v '^(no users)$' | wc -l)" -eq 0 ]]; then
  echo ">> no users found — creating an admin account"
  read -rp "  username: " ADMIN
  sudo -u "$USER_NAME" "$BIN_DST" user-add "$ADMIN" --db "$DATA_DIR/db.sqlite"
fi

# 6. enable + start
systemctl daemon-reload
systemctl enable --now vps-monitor.service

echo ""
echo "Done. Service started."
echo ""
echo "From your local machine, open an SSH tunnel:"
echo "  ssh -L 8443:localhost:8443 <user>@$(hostname -f 2>/dev/null || hostname)"
echo ""
echo "Then visit  http://localhost:8443  in your browser."
echo ""
echo "Logs:    journalctl -u vps-monitor -f"
echo "Config:  $CFG_DIR/config.toml"
echo "Data:    $DATA_DIR/"
