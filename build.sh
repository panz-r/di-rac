#!/bin/bash
set -e

cd "$(dirname "$0")"

mkdir -p dist

# Helper to check for command existence
check_tool() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "ERROR: Tool '$1' is required but not found."
        return 1
    fi
}

echo "==> Checking build dependencies..."
MISSING_TOOLS=0
check_tool cmake || MISSING_TOOLS=1
check_tool cc || MISSING_TOOLS=1
check_tool cargo || MISSING_TOOLS=1
check_tool npm || MISSING_TOOLS=1

if [ $MISSING_TOOLS -ne 0 ]; then
    echo "ERROR: Some build dependencies are missing. Please install them and try again."
    exit 1
fi
echo "    OK"

# Submodule sync only when explicitly requested via './build.sh submodules'
if [ "${1:-}" = "submodules" ]; then
    echo "==> Updating git submodules..."
    git submodule update --init --recursive --remote
    echo "    OK"
fi

echo "==> Building C command daemon..."
(cd command-daemon && cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build)
cp command-daemon/build/di-rvv-cmd dist/di-rvv-cmd
echo "    OK"

echo "==> Building draugr library..."
(cd draugr && cmake -B build -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTS=OFF && cmake --build build)
echo "    OK"

echo "==> Building C central coordination daemon..."
(cd central-daemon && cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build)
cp central-daemon/build/divrr-central-daemon dist/divrr-central-daemon
echo "    OK"

echo "==> Building C analyzer daemon (tree-sitter)..."
(cd treesitter-daemon && cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build)
cp treesitter-daemon/build/di-rvv-analyzer dist/di-rvv-analyzer
echo "    OK"

echo "==> Building Go api-gateway..."
if command -v go >/dev/null 2>&1; then
    (cd api-gateway && go build -o api-gateway .)
    cp api-gateway/api-gateway dist/api-gateway
    echo "    OK"
else
    echo "    SKIPPED (go not found)"
fi

echo "==> Building di-core (Rust execution engine)..."
(cd di-core && cargo build --release)
cp di-core/target/release/di-core dist/di-core
echo "    OK"

echo "==> Building divrr (Rust TUI)..."
(cd divrr && cargo build --release)
cp divrr/target/release/divrr dist/divrr
echo "    OK"

echo "==> Building TypeScript CLI..."
npm install --silent
npm run build
echo "    OK"

echo "==> Build complete. Binaries are in dist/"
ls -l dist/
