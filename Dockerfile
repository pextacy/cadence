# Multi-stage Dockerfile for the Cadence agent service.
#
# The agent (@cadence/agent) depends on the @cadence/mandate workspace, so the
# builder installs the full npm workspace tree and builds mandate before agent.
# The runtime stage ships only the compiled JS + production node_modules and runs
# as a non-root user. The container entrypoint fails fast via the agent's env
# preflight (validateAgentEnv) before the loop starts.

# -----------------------------------------------------------------------------
# Stage 1: builder
# -----------------------------------------------------------------------------
FROM node:22-alpine AS builder
WORKDIR /app

# Root manifests first for cacheable dependency installs.
COPY package.json package-lock.json tsconfig.base.json ./

# All four workspace manifests: `npm ci --workspaces` validates the lockfile
# against every workspace it references, so all must be present even though we
# only build mandate + agent below.
COPY mandate/package.json ./mandate/package.json
COPY agent/package.json ./agent/package.json
COPY scripts/package.json ./scripts/package.json
COPY dashboard/package.json ./dashboard/package.json

# Install the full workspace dev+prod deps so tsc and the workspace link exist.
# --ignore-scripts: no contract/native build steps run in this image.
RUN npm ci --workspaces --include-workspace-root --ignore-scripts

# Source for the two TypeScript workspaces we build.
COPY mandate ./mandate
COPY agent ./agent

# Build mandate first (agent imports its compiled output), then the agent.
RUN npm run build -w @cadence/mandate \
 && npm run build -w @cadence/agent

# Produce a pruned production-only dependency tree for the runtime stage.
RUN npm ci --workspaces --include-workspace-root --omit=dev --ignore-scripts

# -----------------------------------------------------------------------------
# Stage 2: runtime
# -----------------------------------------------------------------------------
FROM node:22-alpine AS runtime
ENV NODE_ENV=production
WORKDIR /app

# Non-root user for the running service.
RUN addgroup -S cadence && adduser -S cadence -G cadence

# Workspace manifests + pruned prod node_modules (symlinked workspace packages).
COPY --from=builder /app/package.json ./package.json
COPY --from=builder /app/node_modules ./node_modules
COPY --from=builder /app/mandate/package.json ./mandate/package.json
COPY --from=builder /app/mandate/dist ./mandate/dist
COPY --from=builder /app/agent/package.json ./agent/package.json
COPY --from=builder /app/agent/dist ./agent/dist

USER cadence

# Health endpoint exposed by the agent's ops/health-server (default port).
EXPOSE 8080

# index.js runs validateAgentEnv() (preflight) before dispatching the loop, so a
# missing/invalid env var exits non-zero with a single structured log line.
CMD ["node", "agent/dist/index.js"]
