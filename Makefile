# ── tool versions ──────────────────────────────────────────────────────────────
FLATC_VERSION  := 23.5.26
PROTOC_VERSION := 28.3

# ── paths ──────────────────────────────────────────────────────────────────────
TOOLS_DIR   := .tools
FLATC       := $(TOOLS_DIR)/flatc
PROTOC      := $(TOOLS_DIR)/protoc
SCHEMA      := schema/messages.fbs
GEN_FLATBUF := src/flatbuf/messages_generated.rs

PYTHON_DIR  := clients/python
PYTHON_VENV := $(PYTHON_DIR)/.venv
PYTHON      := $(PYTHON_VENV)/bin/python3

# ── default goal ───────────────────────────────────────────────────────────────
.DEFAULT_GOAL := all
.PHONY: all server client flatbuf python e2e-deps test test-unit test-integration test-e2e tools clean clean-all help

all: tools flatbuf server client python ## Build everything

# ── tool installation ──────────────────────────────────────────────────────────
tools: $(FLATC) $(PROTOC) ## Install flatc and protoc into .tools/ if absent

$(TOOLS_DIR):
	@mkdir -p $@

## flatc — FlatBuffers compiler
$(FLATC): | $(TOOLS_DIR)
	@if command -v flatc >/dev/null 2>&1; then \
	  echo "→ flatc found on PATH, linking into $(TOOLS_DIR)"; \
	  ln -sf "$$(command -v flatc)" $@; \
	else \
	  echo "→ flatc not found — downloading v$(FLATC_VERSION)"; \
	  curl -fsSL "https://github.com/google/flatbuffers/releases/download/v$(FLATC_VERSION)/Linux.flatc.binary.clang++-12.zip" \
	       -o /tmp/flatc.zip; \
	  unzip -q -o /tmp/flatc.zip -d $(TOOLS_DIR); \
	  chmod +x $@; \
	  rm /tmp/flatc.zip; \
	fi
	@echo "flatc: $$($(FLATC) --version)"

## protoc — Protocol Buffer compiler (required by etcd-client build)
$(PROTOC): | $(TOOLS_DIR)
	@if command -v protoc >/dev/null 2>&1; then \
	  echo "→ protoc found on PATH, linking into $(TOOLS_DIR)"; \
	  ln -sf "$$(command -v protoc)" $@; \
	else \
	  echo "→ protoc not found — downloading v$(PROTOC_VERSION)"; \
	  curl -fsSL "https://github.com/protocolbuffers/protobuf/releases/download/v$(PROTOC_VERSION)/protoc-$(PROTOC_VERSION)-linux-x86_64.zip" \
	       -o /tmp/protoc.zip; \
	  unzip -q -o /tmp/protoc.zip bin/protoc -d /tmp/protoc-extract; \
	  cp /tmp/protoc-extract/bin/protoc $@; \
	  chmod +x $@; \
	  rm -rf /tmp/protoc.zip /tmp/protoc-extract; \
	fi
	@echo "protoc: $$($(PROTOC) --version)"

# ── FlatBuffers code generation ────────────────────────────────────────────────
flatbuf: $(GEN_FLATBUF) ## Generate Rust bindings from schema/messages.fbs

$(GEN_FLATBUF): $(SCHEMA) $(FLATC)
	@echo "→ generating FlatBuffers Rust bindings"
	$(FLATC) --rust -o src/flatbuf $(SCHEMA)
	@echo "generated: $@"

# ── Rust binaries ──────────────────────────────────────────────────────────────
server: $(PROTOC) ## Build the server binary
	PROTOC=$(abspath $(PROTOC)) cargo build --bin server

client: $(PROTOC) ## Build the Rust client binary
	PROTOC=$(abspath $(PROTOC)) cargo build --bin client

# ── Python project (uv + pyproject.toml) ─────────────────────────────────────
python: $(PYTHON) ## Sync Python venv via uv (installs all deps from pyproject.toml)

$(PYTHON): $(PYTHON_DIR)/pyproject.toml
	@echo "→ syncing Python project in $(PYTHON_DIR)"
	cd $(PYTHON_DIR) && uv sync
	@touch $@

e2e-deps: $(PYTHON) ## Install e2e extras (locust) into the Python venv
	cd $(PYTHON_DIR) && uv sync --extra e2e

# ── tests ──────────────────────────────────────────────────────────────────────
test: $(PROTOC) ## Run Rust unit tests + Python unit tests
	PROTOC=$(abspath $(PROTOC)) cargo test
	cd $(PYTHON_DIR) && uv run pytest tests/unit/ -v

test-unit: python ## Run Python unit tests only
	cd $(PYTHON_DIR) && uv run pytest tests/unit/ -v

test-integration: python ## Run Python integration tests (requires running server)
	cd $(PYTHON_DIR) && uv run pytest tests/integration/ -v

test-e2e: e2e-deps ## Run locust e2e tests headlessly (30 s, 20 users)
	cd $(PYTHON_DIR) && uv run locust \
	  -f tests/e2e/locustfile.py \
	  --headless -u 20 -r 5 --run-time 30s \
	  --host http://127.0.0.1:8080

# ── cleanup ────────────────────────────────────────────────────────────────────
clean: ## Remove Rust build artifacts and Python venv
	cargo clean
	rm -rf $(PYTHON_VENV)

clean-all: clean ## Remove everything including downloaded tools
	rm -rf $(TOOLS_DIR)

# ── help ───────────────────────────────────────────────────────────────────────
help: ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) | \
	  awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'
