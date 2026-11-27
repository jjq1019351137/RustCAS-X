# ---------- frontend build ----------
FROM node:22-alpine AS frontend-builder
WORKDIR /fe
RUN corepack enable
COPY frontend/package.json frontend/pnpm-lock.yaml frontend/pnpm-workspace.yaml /fe/
RUN pnpm install --frozen-lockfile
COPY frontend/ /fe/
RUN pnpm build

# ---------- backend build ----------
FROM rust:1-bookworm AS backend-builder
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY backend/Cargo.toml backend/Cargo.lock /src/
COPY backend/migrations /src/migrations
# Dependency layer cache: stub binary first, then rebuild with real sources
RUN mkdir -p src && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --release \
    && rm -rf src
COPY backend/src /src/src
RUN touch src/main.rs \
    && cargo build --release \
    && cp target/release/cas_backend /usr/local/bin/cas_backend

# ---------- runtime ----------
FROM debian:bookworm-slim
ARG BUILD_SHA
ARG BUILD_TAG
ENV BUILD_SHA=$BUILD_SHA \
    BUILD_TAG=$BUILD_TAG \
    TZ=Asia/Shanghai \
    APP_DATA_DIR=/app/backend/data \
    SQLITE_PATH=/app/backend/data/app.db \
    LISTEN_ADDR=0.0.0.0:8000

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends nginx supervisor ca-certificates tzdata \
    && ln -sf /usr/share/zoneinfo/Asia/Shanghai /etc/localtime \
    && mkdir -p /run/nginx /var/log/supervisor /app/backend/data \
    && rm -rf /var/lib/apt/lists/*

COPY --from=backend-builder /usr/local/bin/cas_backend /usr/local/bin/cas_backend
COPY --from=frontend-builder /fe/dist /usr/share/nginx/html
COPY default /etc/nginx/sites-available/default
RUN rm -f /etc/nginx/sites-enabled/default \
    && ln -s /etc/nginx/sites-available/default /etc/nginx/sites-enabled/default

RUN printf '%s\n' \
    '[unix_http_server]' \
    'file=/run/supervisor.sock' \
    'chmod=0700' \
    '' \
    '[rpcinterface:supervisor]' \
    'supervisor.rpcinterface_factory = supervisor.rpcinterface:make_main_rpcinterface' \
    '' \
    '[supervisord]' \
    'nodaemon=true' \
    '' \
    '[program:api]' \
    'directory=/app/backend' \
    'command=/usr/local/bin/cas_backend' \
    'autostart=true' \
    'autorestart=true' \
    'stdout_logfile=/dev/fd/1' \
    'stdout_logfile_maxbytes=0' \
    'stderr_logfile=/dev/fd/2' \
    'stderr_logfile_maxbytes=0' \
    '' \
    '[program:nginx]' \
    'command=/usr/sbin/nginx -g "daemon off;"' \
    'autostart=true' \
    'autorestart=true' \
    'stdout_logfile=/dev/fd/1' \
    'stdout_logfile_maxbytes=0' \
    'stderr_logfile=/dev/fd/2' \
    'stderr_logfile_maxbytes=0' \
    '' \
    '[supervisorctl]' \
    'serverurl=unix:///run/supervisor.sock' \
    > /etc/supervisord.conf

EXPOSE 5115
CMD ["/usr/bin/supervisord","-c","/etc/supervisord.conf"]
