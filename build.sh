#!/bin/bash
set -e

cd "$(dirname "$0")"

mkdir -p dist

echo "==> Building C command daemon..."
(cd command-daemon && cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build)
cp command-daemon/build/di-rvv-cmd dist/di-rvv-cmd 2>/dev/null || true
echo "    OK"

echo "==> Building C central coordination daemon..."
(cd central-deamon && cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build)
cp central-deamon/build/di-vrr-central-deamon dist/di-vrr-central-deamon 2>/dev/null || true
echo "    OK"

echo "==> Building Go api-gateway..."
if command -v go >/dev/null 2>&1; then
    (cd api-gateway && go build -o api-gateway .)
    cp api-gateway/api-gateway dist/api-gateway 2>/dev/null || true
    echo "    OK"
else
    echo "    SKIPPED (go not found)"
fi

echo "==> Building Rust tree-sitter analyzer..."
if command -v cargo >/dev/null 2>&1; then
    (cd treesitter-daemon && cargo build --release)
    cp treesitter-daemon/target/release/di-rvv-analyzer dist/di-rvv-analyzer 2>/dev/null || true
    echo "    OK"
else
    echo "    SKIPPED (cargo not found)"
fi

echo "==> Building TypeScript CLI..."
if [ -f "package.json" ]; then
    npm install --silent
    npm run build
    echo "    OK"
else
    echo "    SKIPPED (package.json not found)"
fi

echo "==> Build complete. Binaries are in dist/"
ls -l dist/
