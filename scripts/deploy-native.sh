#!/usr/bin/env bash
# Run as root on the verified, dedicated Ubuntu host with a staged release archive.
set -euo pipefail
archive=${1:?release archive required}
host=${2:?public DNS hostname required}
expected=${3:?archive SHA256 required}
[[ "$host" =~ ^[a-zA-Z0-9.-]+$ ]] || exit 2
echo "$expected  $archive" | sha256sum -c -
stamp=$(date -u +%Y%m%dT%H%M%SZ)
release="/opt/vision-pro/releases/$stamp"
backup="/opt/vision-pro/backups/$stamp"
install -d -m 755 "$release" "$backup" /var/www/vision-pro-acme
cp -a /etc/nginx "$backup/nginx"
if [ -L /opt/vision-pro/current ]; then readlink -f /opt/vision-pro/current > "$backup/previous-release"; fi
tar -xzf "$archive" -C "$release"
chmod 755 "$release/vp-relay"
if [ ! -x /opt/vision-pro/node/bin/node ]; then
  install -d /opt/vision-pro/node
  download=$(mktemp -d)
  curl --fail --silent --show-error --location https://nodejs.org/dist/v22.23.4/node-v22.23.4-linux-x64.tar.xz -o "$download/node-v22.23.4-linux-x64.tar.xz"
  curl --fail --silent --show-error --location https://nodejs.org/dist/v22.23.4/SHASUMS256.txt -o "$download/SHASUMS256.txt"
  (cd "$download" && grep ' node-v22.23.4-linux-x64.tar.xz$' SHASUMS256.txt | sha256sum -c -)
  tar -xJf "$download/node-v22.23.4-linux-x64.tar.xz" --strip-components=1 -C /opt/vision-pro/node
fi
export PATH="/opt/vision-pro/node/bin:$PATH"
(cd "$release/server" && npm ci --omit=dev --no-audit --no-fund)
id vision-pro >/dev/null 2>&1 || useradd --system --home /var/lib/vision-pro --shell /usr/sbin/nologin vision-pro
install -d -o vision-pro -g vision-pro -m 700 /var/lib/vision-pro
install -d -m 700 /etc/vision-pro
cat > /etc/vision-pro/account.env <<EOF
VP_ACCOUNT_PORT=3000
VP_ACCOUNT_DATA=/var/lib/vision-pro/users.json
VP_WEB_ROOT=/opt/vision-pro/current/web
VP_ALLOW_REGISTRATION=1
VP_TRUST_PROXY=1
NODE_ENV=production
EOF
chmod 600 /etc/vision-pro/account.env
cat > /etc/systemd/system/vision-pro-account.service <<'EOF'
[Unit]
Description=Vision-Pro accounts and iPad app
After=network-online.target
Wants=network-online.target
[Service]
User=vision-pro
Group=vision-pro
WorkingDirectory=/opt/vision-pro/current/server
EnvironmentFile=/etc/vision-pro/account.env
ExecStart=/opt/vision-pro/node/bin/node --experimental-strip-types src/index.ts
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/vision-pro
UMask=0077
MemoryMax=320M
TasksMax=64
[Install]
WantedBy=multi-user.target
EOF
cat > /etc/systemd/system/vision-pro-relay.service <<EOF
[Unit]
Description=Vision-Pro encrypted Internet relay
After=network-online.target
Wants=network-online.target
[Service]
User=vision-pro
Group=vision-pro
ExecStart=/opt/vision-pro/current/vp-relay --host $host --http 127.0.0.1:18080 --turn 127.0.0.1:13478 --trust-proxy
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
UMask=0077
MemoryMax=192M
TasksMax=64
[Install]
WantedBy=multi-user.target
EOF
ln -sfn "$release" /opt/vision-pro/current
systemctl daemon-reload
systemctl enable --now vision-pro-account vision-pro-relay
systemctl restart vision-pro-account vision-pro-relay
for i in $(seq 1 30); do
  if curl -fsS http://127.0.0.1:3000/api/account/config && curl -fsS http://127.0.0.1:18080/healthz; then break; fi
  if [ "$i" = 30 ]; then echo 'readiness failed; inspect services' >&2; exit 1; fi
  sleep 1
done
cat > /etc/nginx/sites-available/vision-pro <<EOF
server {
  listen 80;
  server_name $host;
  location /.well-known/acme-challenge/ { root /var/www/vision-pro-acme; }
  location / { return 301 https://\$host\$request_uri; }
}
EOF
ln -sfn /etc/nginx/sites-available/vision-pro /etc/nginx/sites-enabled/vision-pro
nginx -t
systemctl enable --now nginx
systemctl reload nginx
certbot certonly --non-interactive --agree-tos --register-unsafely-without-email --webroot -w /var/www/vision-pro-acme -d "$host"
cat >> /etc/nginx/sites-available/vision-pro <<EOF
server {
  listen 443 ssl;
  server_name $host;
  ssl_certificate /etc/letsencrypt/live/$host/fullchain.pem;
  ssl_certificate_key /etc/letsencrypt/live/$host/privkey.pem;
  ssl_protocols TLSv1.2 TLSv1.3;
  client_max_body_size 32k;
  server_tokens off;
  location /v1/ {
    proxy_pass http://127.0.0.1:18080;
    proxy_http_version 1.1;
    proxy_set_header Upgrade \$http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header Host \$host;
    proxy_set_header X-Forwarded-For \$remote_addr;
    proxy_read_timeout 90s;
    proxy_send_timeout 90s;
  }
  location = /healthz { proxy_pass http://127.0.0.1:18080; }
  location / {
    proxy_pass http://127.0.0.1:3000;
    proxy_set_header Host \$host;
    proxy_set_header X-Forwarded-For \$remote_addr;
    proxy_set_header X-Forwarded-Proto https;
  }
}
EOF
nginx -t
systemctl reload nginx
install -d /etc/letsencrypt/renewal-hooks/deploy
printf '#!/bin/sh\nsystemctl reload nginx\n' > /etc/letsencrypt/renewal-hooks/deploy/vision-pro-nginx
chmod 755 /etc/letsencrypt/renewal-hooks/deploy/vision-pro-nginx
systemctl enable --now certbot.timer
curl --fail --silent --show-error "https://$host/api/account/config"
echo
echo "RELEASE=$release BACKUP=$backup HOST=$host"
systemctl show vision-pro-account vision-pro-relay -p ActiveState -p NRestarts
