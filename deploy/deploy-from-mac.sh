#!/usr/bin/env bash
#
# Deploy the Cubic Data Dashboard to the VPS.
#
#   VPS=shaksham@97.74.95.208 DOMAIN=cubic-data.com bash deploy/deploy-from-mac.sh
#
# Run from cubic-data-team/. Idempotent: safe to re-run for every deploy.
#
# Live production on this box (do not invent new paths):
#   App dir : /opt/cubic-data-team
#   Service : cubic-data-team.service  (port 3010)
#   Nginx   : /etc/nginx/sites-enabled/cubic-data-domain.conf  (TLS via certbot)
#
# Secrets are NOT synced by a normal deploy. Push them once with:
#   VPS=user@host bash deploy/deploy-from-mac.sh --push-secrets
set -euo pipefail

VPS="${VPS:?Set VPS=user@host}"
# Match the already-running production layout on cubic-data.com.
APP_DIR="${APP_DIR:-/opt/cubic-data-team}"
SERVICE="${SERVICE:-cubic-data-team}"
PORT="${PORT:-3010}"
# Never rewrite the certbot TLS vhost unless explicitly requested.
SKIP_NGINX="${SKIP_NGINX:-1}"

HERE="$(cd "$(dirname "$0")" && pwd)"
APP_SRC="$(dirname "$HERE")"
DEPLOY_USER="${VPS%@*}"

# GoDaddy / flaky VPS links drop idle SSH mid-transfer. Use one multiplexed
# connection + keepalives so retries do not pile up zombie sshd/sftp sessions.
# macOS $TMPDIR (/var/folders/...) overflows the 104-char unix socket limit,
# so keep the control path short and predictable.
MUX_DIR="${CUBIC_SSH_MUX_DIR:-/tmp/cubic-ssh-mux}"
mkdir -p "$MUX_DIR"
CTRL="$MUX_DIR/%r@%h:%p"
# Git Bash / MSYS cannot mux over Unix sockets; use a fresh SSH per call.
if [[ "${OSTYPE:-}" == msys* || "${OSTYPE:-}" == cygwin* || "$(uname -s 2>/dev/null)" == MINGW* ]]; then
  SSH_OPTS=(
    -o BatchMode=yes
    -o IdentitiesOnly=yes
    -o IdentityFile="$HOME/.ssh/id_ed25519"
    -o ServerAliveInterval=15
    -o ServerAliveCountMax=120
    -o TCPKeepAlive=yes
    -o ConnectTimeout=45
  )
  cleanup_mux() { true; }
else
  SSH_OPTS=(
    -o BatchMode=yes
    -o ControlMaster=auto
    -o ControlPath="$CTRL"
    -o ControlPersist=600
    -o ServerAliveInterval=15
    -o ServerAliveCountMax=120
    -o TCPKeepAlive=yes
    -o ConnectTimeout=45
  )
  cleanup_mux() {
    ssh -O exit -o ControlPath="$CTRL" "$VPS" 2>/dev/null || true
  }
fi
ssh_vps() { ssh "${SSH_OPTS[@]}" "$VPS" "$@"; }
scp_vps() { scp "${SSH_OPTS[@]}" "$@"; }
trap cleanup_mux EXIT

# Open the master once; fail fast if the box is unreachable.
echo "==> Opening SSH mux to $VPS"
ssh_vps "echo mux_ok; hostname"

# Drop leftover notty/sftp sessions from prior broken deploys (keeps the app up).
ssh_vps 'bash -s' <<'REMOTE' || true
set +e
ps -eo pid=,etime=,cmd= | grep -E "sftp-server|sshd: [^ ]+@notty" | grep -v grep | while read -r pid etime cmd; do
  secs=0
  if [[ "$etime" == *-* ]]; then
    days=${etime%%-*}; rest=${etime#*-}
    IFS=: read -r h m s <<<"$rest"
    secs=$((10#$days*86400 + 10#$h*3600 + 10#$m*60 + 10#$s))
  else
    IFS=: read -r a b c <<<"$etime"
    if [[ -n "${c:-}" ]]; then secs=$((10#$a*3600 + 10#$b*60 + 10#$c))
    else secs=$((10#$a*60 + 10#$b)); fi
  fi
  if (( secs > 180 )); then kill "$pid" 2>/dev/null; fi
done
true
REMOTE

# Upload a local file to $remote_path with retries; falls back to 256KB chunks.
upload_file() {
  local src="$1" remote_path="$2"
  local attempt
  for attempt in 1 2 3; do
    if scp_vps "$src" "$VPS:$remote_path"; then
      return 0
    fi
    echo "    scp failed (attempt $attempt); retrying..."
    sleep $((attempt * 3))
    # Re-open mux if the master died.
    ssh_vps "true" >/dev/null 2>&1 || true
  done

  echo "    falling back to chunked upload (256KB)"
  local chunk_dir remote_chunks base
  chunk_dir="$(mktemp -d "${TMPDIR:-/tmp}/cubic-chunks.XXXXXX")"
  remote_chunks="/tmp/cubic-deploy-chunks-$$"
  split -b 256k "$src" "$chunk_dir/part_"
  ssh_vps "rm -rf '$remote_chunks' && mkdir -p '$remote_chunks'"
  for f in "$chunk_dir"/part_*; do
    base="$(basename "$f")"
    for attempt in 1 2 3 4 5; do
      if scp_vps "$f" "$VPS:$remote_chunks/$base"; then
        break
      fi
      echo "    RETRY $base attempt=$attempt"
      sleep $((attempt * 2))
      ssh_vps "true" >/dev/null 2>&1 || true
      if [[ $attempt -eq 5 ]]; then
        rm -rf "$chunk_dir"
        ssh_vps "rm -rf '$remote_chunks'" || true
        echo "chunked upload failed for $base" >&2
        return 1
      fi
    done
  done
  ssh_vps "cat '$remote_chunks'/part_* > '$remote_path' && rm -rf '$remote_chunks' && gzip -t '$remote_path'"
  rm -rf "$chunk_dir"
}

# ---------------------------------------------------------------- push-secrets
if [[ "${1:-}" == "--push-secrets" ]]; then
  echo "==> Pushing secrets to $VPS:$APP_DIR (mode 600)"
  ssh_vps "sudo mkdir -p '$APP_DIR' && sudo chown '$DEPLOY_USER' '$APP_DIR'"
  for f in .env.production service-account-key.json gmail-oauth.json .calendar_oauth_token.json; do
    if [[ -f "$APP_SRC/$f" ]]; then
      upload_file "$APP_SRC/$f" "$APP_DIR/$f"
      ssh_vps "chmod 600 '$APP_DIR/$f'"
      echo "    sent $f"
    else
      echo "    SKIP $f (not found locally)"
    fi
  done
  echo "==> Done. Secrets are not overwritten by a normal deploy."
  exit 0
fi

DOMAIN="${DOMAIN:?Set DOMAIN=cubic-data.com (used only if SKIP_NGINX=0)}"

echo "==> Target   : $VPS"
echo "==> App dir  : $APP_DIR"
echo "==> Service  : $SERVICE  (port $PORT)"
echo "==> Domain   : $DOMAIN"
echo "==> Nginx    : $([[ "$SKIP_NGINX" == "1" ]] && echo 'SKIP (keep cubic-data-domain.conf)' || echo 'REWRITE')"
echo

# ------------------------------------------------------------------ 1. sources
echo "==> Syncing source"
ssh_vps "sudo mkdir -p '$APP_DIR' && sudo chown '$DEPLOY_USER' '$APP_DIR'"
# Prefer a compressed tar — more reliable than rsync over flaky VPS links.
TMP_TAR="$(mktemp -t cubic-deploy.XXXXXX.tgz)"
cleanup_all() { rm -f "$TMP_TAR"; cleanup_mux; }
trap cleanup_all EXIT
tar -C "$APP_SRC" -czf "$TMP_TAR" \
  --exclude='.git' \
  --exclude='node_modules' \
  --exclude='.next' \
  --exclude='backend/target' \
  --exclude='._*' \
  --exclude='.env' \
  --exclude='.env.*' \
  --exclude='service-account-key.json' \
  --exclude='gmail-oauth.json' \
  --exclude='.calendar_oauth_token.json' \
  --exclude='oauth-web.json' \
  --exclude='data' \
  --exclude='tsconfig.tsbuildinfo' \
  .
ls -lh "$TMP_TAR"
upload_file "$TMP_TAR" /tmp/cubic-data-team-deploy.tgz
ssh_vps "mkdir -p '$APP_DIR' && tar -xzf /tmp/cubic-data-team-deploy.tgz -C '$APP_DIR' && rm -f /tmp/cubic-data-team-deploy.tgz"

# ------------------------------------------------------------------- 2. deps
echo "==> Installing Node 20 if missing, then dependencies"
ssh_vps bash -se <<'REMOTE'
set -euo pipefail
if ! command -v node >/dev/null 2>&1 || [[ "$(node -v | cut -c2- | cut -d. -f1)" -lt 20 ]]; then
  curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
  sudo apt-get install -y nodejs
fi
node -v
REMOTE

# Dev deps are required: typescript + tailwind run at build time.
ssh_vps "cd '$APP_DIR' && npm ci --include=dev"

# ------------------------------------------------------------------- 3. build
echo "==> Building Next.js static UI & Rust backend (on VPS)"
ssh_vps bash -se <<REMOTE
set -euo pipefail
export PATH="\$HOME/.cargo/bin:\$PATH"
cd '$APP_DIR'
# One build id for the HTML, the JS bundle and version.json, so open tabs can tell
# reliably when they are running an older build.
CUBIC_APP_VERSION="b$(date +%s%3N)" NODE_ENV=production npm run build
cd '$APP_DIR/backend'
cargo build --release
REMOTE

# ------------------------------------------------------------- 4. env present?
ssh_vps "test -s '$APP_DIR/.env.production'" || {
  echo
  echo "!! $APP_DIR/.env.production is missing or empty."
  echo "   Fill deploy/.env.production.example locally as .env.production, then:"
  echo "     VPS=$VPS bash deploy/deploy-from-mac.sh --push-secrets"
  echo "   The app throws on boot without AUTH_JWT_SECRET."
  exit 1
}

# ------------------------------------------------------------------ 5. systemd
echo "==> Installing systemd unit ($SERVICE)"
sed \
  -e "s|__DEPLOY_USER__|$DEPLOY_USER|g" \
  -e "s|/opt/cubic-data-team|$APP_DIR|g" \
  -e "s|RUST_BACKEND_PORT=3010|RUST_BACKEND_PORT=$PORT|g" \
  "$HERE/cubic-dashboard.service" \
  | ssh_vps "sudo tee /etc/systemd/system/$SERVICE.service >/dev/null"

# Preserve drop-in overrides (CRON_SECRET, etc.) if present.
ssh_vps "sudo systemctl daemon-reload && sudo systemctl enable '$SERVICE' && sudo systemctl restart '$SERVICE'"

# -------------------------------------------------------------------- 6. nginx
if [[ "$SKIP_NGINX" == "1" ]]; then
  echo "==> Skipping nginx rewrite (live TLS vhost stays in place)"
else
  echo "==> Installing nginx vhost (HTTP-only template — prefer certbot-managed file)"
  sed "s|__DOMAIN__|$DOMAIN|g; s|127.0.0.1:3100|127.0.0.1:$PORT|g" \
    "$HERE/nginx-cubic-dashboard.conf" \
    | ssh_vps "sudo tee /etc/nginx/sites-available/$SERVICE >/dev/null"
  ssh_vps "sudo ln -sfn /etc/nginx/sites-available/$SERVICE /etc/nginx/sites-enabled/$SERVICE"
  echo "==> nginx -t (aborts before reload if the config is bad)"
  ssh_vps "sudo nginx -t"
  ssh_vps "sudo systemctl reload nginx"
fi

# ------------------------------------------------------------------- 7. verify
echo "==> Verifying"
sleep 4
ssh_vps "systemctl is-active '$SERVICE'" || true
echo -n "    local /login           -> HTTP "
ssh_vps "curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:$PORT/login" || true
echo
echo -n "    public /login          -> HTTP "
curl -s -o /dev/null -w '%{http_code}' --connect-timeout 15 "https://$DOMAIN/login" || true
echo
echo -n "    public /book/phone-call -> HTTP "
curl -s -o /dev/null -w '%{http_code}' --connect-timeout 15 "https://$DOMAIN/book/phone-call" || true
echo
echo -n "    public /book/appointment -> HTTP "
curl -s -o /dev/null -w '%{http_code}' --connect-timeout 15 "https://$DOMAIN/book/appointment" || true
echo
cat <<EOF

==> Deployed to $VPS:$APP_DIR ($SERVICE :$PORT)

Logs:  ssh $VPS 'sudo journalctl -u $SERVICE -f'
EOF
