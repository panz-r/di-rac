.PHONY: all build build-api-gateway build-command-daemon build-treesitter-daemon build-divrr build-di-core clean install install-divrr install-di-core

DIST := bin

# Build all production binaries
all: build

build: build-api-gateway build-command-daemon build-treesitter-daemon build-draugr build-central-daemon build-divrr build-di-core build-wasm-runner build-shims bundle-python

# ---------------------------------------------------------------------------
# draugr (C) — high-performance library
# ---------------------------------------------------------------------------
build-draugr:
	@echo "  BUILD   draugr"
	cd draugr && cmake -B build -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTS=OFF 2>&1 | tail -1
	cd draugr && cmake --build build 2>&1
	@echo "  DONE    draugr"

# ---------------------------------------------------------------------------
# central-daemon (C) — task coordination
# ---------------------------------------------------------------------------
build-central-daemon:
	@echo "  BUILD   central-daemon"
	@mkdir -p $(DIST)
	cd central-daemon && cmake -B build -DCMAKE_BUILD_TYPE=Release 2>&1 | tail -1
	cd central-daemon && cmake --build build 2>&1 && cp build/divrr-central-daemon ../$(DIST)/divrr-central-daemon
	@chmod 755 $(DIST)/divrr-central-daemon
	@echo "  DONE    $(DIST)/divrr-central-daemon"

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
	cd command-daemon && cmake --build build 2>&1 && cp build/di-rvv-cmd ../$(DIST)/di-rvv-cmd
	@chmod 755 $(DIST)/di-rvv-cmd
	@echo "  DONE    $(DIST)/di-rvv-cmd"

# ---------------------------------------------------------------------------
# treesitter-daemon (C) — AST analysis
# ---------------------------------------------------------------------------
build-treesitter-daemon:
	@echo "  BUILD   treesitter-daemon"
	@mkdir -p $(DIST)
	cd treesitter-daemon && cmake -B build -DCMAKE_BUILD_TYPE=Release 2>&1 | tail -1
	cd treesitter-daemon && cmake --build build 2>&1 && cp build/divrr-analyzer ../$(DIST)/divrr-analyzer
	@chmod 755 $(DIST)/divrr-analyzer
	@echo "  DONE    $(DIST)/divrr-analyzer"

# ---------------------------------------------------------------------------
# divrr (Rust) — TUI frontend
# ---------------------------------------------------------------------------
build-divrr:
	@echo "  BUILD   divrr"
	@mkdir -p $(DIST)
	cargo build --release --manifest-path divrr/Cargo.toml 2>&1 && cp divrr/target/release/divrr $(DIST)/divrr
	@chmod 755 $(DIST)/divrr
	@echo "  DONE    $(DIST)/divrr"

# ---------------------------------------------------------------------------
# di-core (Rust) — agent engine
# ---------------------------------------------------------------------------
build-di-core:
	@echo "  BUILD   di-core"
	@mkdir -p $(DIST)
	cargo build --release --manifest-path di-core/Cargo.toml 2>&1 && cp di-core/target/release/di-core $(DIST)/di-core
	@chmod 755 $(DIST)/di-core
	@echo "  DONE    $(DIST)/di-core"

# ---------------------------------------------------------------------------
# wasm-runner (Rust) — Wasm sandbox for Python
# ---------------------------------------------------------------------------
build-wasm-runner:
	@echo "  BUILD   wasm-runner"
	@mkdir -p $(DIST)
	cargo build --release --manifest-path wasm-runner/Cargo.toml
	cp wasm-runner/target/release/wasm-runner $(DIST)/wasm-runner
	@chmod 755 $(DIST)/wasm-runner
	@# Copy libwasmedge if found in build artifacts
	@WASM_LIB=$$(find wasm-runner/target/release/build -name libwasmedge.so.0 | head -n 1); \
	if [ -n "$$WASM_LIB" ]; then \
		cp "$$WASM_LIB" $(DIST)/; \
		echo "  BUN     libwasmedge.so.0"; \
	fi
	@echo "  DONE    $(DIST)/wasm-runner"

# ---------------------------------------------------------------------------
# shims — Python interception scripts
# ---------------------------------------------------------------------------
build-shims: build-wasm-runner
	@echo "  BUILD   shims"
	@mkdir -p $(DIST)/shims
	@printf "#!/bin/bash\n# di-vrr python sandbox shim\nEXE_DIR=\$$(dirname \"\$$(readlink -f \"\$$0\")\")\nexport LD_LIBRARY_PATH=\"\$${EXE_DIR}/..\":\$$LD_LIBRARY_PATH\n\"\$${EXE_DIR}/../wasm-runner\" --wasm \"\$${EXE_DIR}/../../standalone/runtime-files/python.wasm\" --preopen \"/lib:\$${EXE_DIR}/../../standalone/runtime-files/usr/local/lib\" --preopen \".:.\" -- \"\$$@\"\n" > $(DIST)/shims/python3
	@chmod +x $(DIST)/shims/python3
	@ln -sf python3 $(DIST)/shims/python
	@echo "  DONE    $(DIST)/shims/"

# ---------------------------------------------------------------------------
# python-wasm bundling
# ---------------------------------------------------------------------------
bundle-python:
	@echo "  BUNDLE  python-wasm"
	@if [ ! -f standalone/runtime-files/python.wasm ]; then \
		node scripts/download-wasm-python.mjs; \
	fi
	@if command -v wasmedge >/dev/null 2>&1; then \
		echo "  AOT     python.wasm"; \
		wasmedge compile standalone/runtime-files/python.wasm standalone/runtime-files/python.wasm.aot >/dev/null 2>&1 || true; \
	fi
	@echo "  DONE    python-wasm"

# ---------------------------------------------------------------------------
# Convenience install targets — copies binaries to ~/.di/dist/
# ---------------------------------------------------------------------------
PREFIX ?= $(HOME)/.di

install: build
	@mkdir -p $(PREFIX)/dist
	@mkdir -p $(PREFIX)/standalone
	cp $(DIST)/api-gateway $(PREFIX)/dist/api-gateway
	cp $(DIST)/di-rvv-cmd $(PREFIX)/dist/di-rvv-cmd
	cp $(DIST)/divrr-analyzer $(PREFIX)/dist/divrr-analyzer
	cp $(DIST)/divrr-central-daemon $(PREFIX)/dist/divrr-central-daemon
	cp $(DIST)/divrr $(PREFIX)/dist/divrr
	cp $(DIST)/di-core $(PREFIX)/dist/di-core
	cp $(DIST)/wasm-runner $(PREFIX)/dist/wasm-runner
	@if [ -f $(DIST)/libwasmedge.so.0 ]; then cp $(DIST)/libwasmedge.so.0 $(PREFIX)/dist/; fi
	cp -r $(DIST)/shims $(PREFIX)/dist/
	cp -r standalone/runtime-files $(PREFIX)/standalone/
	@echo "  INSTALL $(PREFIX)/dist/ and $(PREFIX)/standalone/"

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
