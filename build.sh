#!/bin/bash
set -e

cd "$(dirname "$0")"

echo "==> Building C command daemon..."
(cd command-daemon && cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build)
echo "    OK"

echo "==> Building Go api-gateway..."
(cd api-gateway && go build -o api-gateway .)
echo "    OK"

echo "==> Building Rust tree-sitter analyzer..."
(cd treesitter-daemon && cargo build --release)
echo "    OK"

echo "==> Building TypeScript CLI..."
npm run build
echo "    OK"

echo "==> Copying binaries to dist..."
cp command-daemon/build/di-rvv-cmd dist/di-rvv-cmd 2>/dev/null || true
cp api-gateway/api-gateway dist/api-gateway 2>/dev/null || true
cp treesitter-daemon/target/release/di-rvv-analyzer dist/di-rvv-analyzer 2>/dev/null || true
echo "    OK"

echo "==> Build complete"
