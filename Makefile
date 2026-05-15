.PHONY: all build build-api-gateway build-command-daemon build-treesitter-daemon build-divrr build-di-core clean install install-divrr install-di-core

DIST := bin

# Build all production binaries
all: build

build: build-api-gateway build-command-daemon build-treesitter-daemon build-divrr build-di-core

# ---------------------------------------------------------------------------
# api-gateway (Go) — LLM API proxy
# ---------------------------------------------------------------------------
build-api-gateway:
	@echo "  BUILD   api-gateway"
	@mkdir -p $(DIST)
	cd api-gateway && go build -o ../$(DIST)/api-gateway .
	@chmod 755 $(DIST)/api-gateway
	@echo "  DONE    $(DIST)/api-gateway"

# ---------------------------------------------------------------------------
# command-daemon (C) — child process execution
# ---------------------------------------------------------------------------
build-command-daemon:
	@echo "  BUILD   command-daemon"
	@mkdir -p $(DIST)
	cd command-daemon && cmake -B build -DCMAKE_BUILD_TYPE=Release 2>&1 | tail -1
	cd command-daemon && cmake --build build 2>&1 | tail -1
	cp command-daemon/build/di-rvv-cmd $(DIST)/di-rvv-cmd
	@chmod 755 $(DIST)/di-rvv-cmd
	@echo "  DONE    $(DIST)/di-rvv-cmd"

# ---------------------------------------------------------------------------
# treesitter-daemon (C) — AST analysis
# ---------------------------------------------------------------------------
build-treesitter-daemon:
	@echo "  BUILD   treesitter-daemon"
	@mkdir -p $(DIST)
	cd treesitter-daemon && cmake -B build -DCMAKE_BUILD_TYPE=Release 2>&1 | tail -1
	cd treesitter-daemon && cmake --build build 2>&1 | tail -1
	cp treesitter-daemon/build/divrr-analyzer $(DIST)/divrr-analyzer
	@chmod 755 $(DIST)/divrr-analyzer
	@echo "  DONE    $(DIST)/divrr-analyzer"

# ---------------------------------------------------------------------------
# divrr (Rust) — TUI frontend
# ---------------------------------------------------------------------------
build-divrr:
	@echo "  BUILD   divrr"
	@mkdir -p $(DIST)
	cargo build --release --manifest-path divrr/Cargo.toml 2>&1 | tail -1
	cp divrr/target/release/divrr $(DIST)/divrr
	@chmod 755 $(DIST)/divrr
	@echo "  DONE    $(DIST)/divrr"

# ---------------------------------------------------------------------------
# di-core (Rust) — agent engine
# ---------------------------------------------------------------------------
build-di-core:
	@echo "  BUILD   di-core"
	@mkdir -p $(DIST)
	cargo build --release --manifest-path di-core/Cargo.toml 2>&1 | tail -1
	cp di-core/target/release/di-core $(DIST)/di-core
	@chmod 755 $(DIST)/di-core
	@echo "  DONE    $(DIST)/di-core"

# ---------------------------------------------------------------------------
# Convenience install targets — copies binaries to ~/.di/dist/
# ---------------------------------------------------------------------------
PREFIX ?= $(HOME)/.di

install: build
	@mkdir -p $(PREFIX)/dist
	cp $(DIST)/api-gateway $(PREFIX)/dist/api-gateway
	cp $(DIST)/di-rvv-cmd $(PREFIX)/dist/di-rvv-cmd
	cp $(DIST)/divrr-analyzer $(PREFIX)/dist/divrr-analyzer
	cp $(DIST)/divrr $(PREFIX)/dist/divrr
	cp $(DIST)/di-core $(PREFIX)/dist/di-core
	@echo "  INSTALL $(PREFIX)/dist/"

install-divrr: build-divrr
	@mkdir -p $(PREFIX)/dist
	cp $(DIST)/divrr $(PREFIX)/dist/divrr
	@echo "  INSTALL $(PREFIX)/dist/divrr"

install-di-core: build-di-core
	@mkdir -p $(PREFIX)/dist
	cp $(DIST)/di-core $(PREFIX)/dist/di-core
	@echo "  INSTALL $(PREFIX)/dist/di-core"

# ---------------------------------------------------------------------------
# Development helpers
# ---------------------------------------------------------------------------
build-fast: build-api-gateway
	@echo "  (use build-fast for quick gateway iteration)"

# ---------------------------------------------------------------------------
# Clean
# ---------------------------------------------------------------------------
clean:
	rm -rf $(DIST)
	cd api-gateway && go clean 2>/dev/null || true
	cd command-daemon && rm -rf build 2>/dev/null || true
	cd treesitter-daemon && rm -rf build 2>/dev/null || true
	cargo clean --manifest-path divrr/Cargo.toml 2>/dev/null || true
	cargo clean --manifest-path di-core/Cargo.toml 2>/dev/null || true
	@echo "  CLEAN   all builds"
