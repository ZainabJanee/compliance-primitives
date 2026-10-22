# syntax=docker/dockerfile:1

# ── Stage 1: build ────────────────────────────────────────────────────────────
FROM node:20-alpine AS builder

WORKDIR /app

# Install dependencies first (layer-cache friendly)
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts

# Copy source and compile
COPY tsconfig.json ./
COPY src/ ./src/
RUN npm run build

# Prune dev dependencies so only production deps land in the final image
RUN npm prune --omit=dev

# ── Stage 2: runtime ──────────────────────────────────────────────────────────
FROM node:20-alpine AS runtime

WORKDIR /app

# Copy compiled output and production node_modules from builder
COPY --from=builder /app/dist       ./dist
COPY --from=builder /app/node_modules ./node_modules
COPY package.json ./

# The SQLite database file lands here by default (DB_PATH=compliance.db).
# Mount a named volume at /data and set DB_PATH=/data/compliance.db to
# persist the database across container restarts.
VOLUME ["/data"]

# Document the env vars the process needs (values supplied at runtime via
# --env-file or -e flags; no secrets are baked into the image).
ENV RPC_URL=""
ENV NETWORK_PASSPHRASE=""
ENV ALLOWLIST_CONTRACT_ID=""
ENV DENYLIST_CONTRACT_ID=""
ENV JURISDICTION_CONTRACT_ID=""
ENV DB_PATH="/data/compliance.db"
ENV POLL_INTERVAL_MS="5000"
ENV START_LEDGER="0"
# Set HEALTH_PORT to expose the /health and /status HTTP endpoints.
ENV HEALTH_PORT=""

# Non-root user for container security
RUN addgroup -S indexer && adduser -S indexer -G indexer
USER indexer

# Optional liveness probe — only active when HEALTH_PORT is set.
# Override with `docker run --health-cmd` if you use a different port.
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD [ -z "$HEALTH_PORT" ] && exit 0 || wget -qO- http://localhost:${HEALTH_PORT}/health | grep -q '"ok":true' || exit 1

CMD ["node", "dist/index.js"]
