# ── Stage 1: dependency install ───────────────────────────────────────────────
# Copy only the project manifest first so this layer is cached unless
# pyproject.toml changes (source code changes don't bust the dep cache).
FROM python:3.12-slim AS deps

COPY --from=ghcr.io/astral-sh/uv:0.5 /uv /usr/local/bin/uv

WORKDIR /app

COPY pyproject.toml uv.lock* ./
# Install production deps only, skip the project itself (src not copied yet).
RUN uv sync --no-dev --no-install-project

# ── Stage 2: application ───────────────────────────────────────────────────────
FROM deps AS app

COPY src/ src/
# Install the project packages (app + client) into the already-built venv.
RUN uv sync --no-dev

ENV PATH="/app/.venv/bin:$PATH"

# Default rate-limiter coordinates; override at runtime or via docker-compose.
ENV RATE_LIMITER_HOST=rate-limiter
ENV RATE_LIMITER_PORT=9000

EXPOSE 8000

CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
