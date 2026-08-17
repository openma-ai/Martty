RUST_CACHE_MAX_GIB ?= 20
RUST_DISK_MIN_GIB ?= 10
CARGO_ARGS ?= test --locked
RUST_TARGET_DIR ?= $(if $(DSH_TUI_CARGO_TARGET_DIR),$(DSH_TUI_CARGO_TARGET_DIR),$(CURDIR)/target)
TUI_DEBUG_BIN ?= $(RUST_TARGET_DIR)/debug/dsh-tui
REAL_AGENT_E2E_SPEC ?= scripts/real-agent-e2e.config-slider.json
REAL_AGENT_E2E_ARGS ?=

CARGO_GUARD = DSH_TUI_CARGO_TARGET_DIR="$(RUST_TARGET_DIR)" \
	DSH_TUI_RUST_CACHE_MAX_GIB=$(RUST_CACHE_MAX_GIB) \
	DSH_TUI_RUST_DISK_MIN_GIB=$(RUST_DISK_MIN_GIB) \
	./scripts/cargo-guard.sh

.PHONY: rust rust-build rust-test rust-cache-status rust-cache-prune tui-test real-agent-e2e

rust:
	$(CARGO_GUARD) $(CARGO_ARGS)

rust-build:
	$(CARGO_GUARD) build --locked

rust-test:
	$(CARGO_GUARD) test --locked

rust-cache-status:
	$(CARGO_GUARD) status

rust-cache-prune:
	$(CARGO_GUARD) prune

tui-test: rust-build
	DSH_TUI_BIN="$(TUI_DEBUG_BIN)" dsh --profile tui-test

# Opt-in: drives the real tui-test/ACP/Creator stack and consumes model tokens.
# Override REAL_AGENT_E2E_SPEC or pass generic runner flags via REAL_AGENT_E2E_ARGS.
real-agent-e2e: rust-build
	DSH_TUI_BIN="$(TUI_DEBUG_BIN)" python3 ./scripts/real-agent-e2e.py run \
		--spec "$(REAL_AGENT_E2E_SPEC)" $(REAL_AGENT_E2E_ARGS)
