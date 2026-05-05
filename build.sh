#!/bin/bash
set -e

cd "$(dirname "$0")"

echo "==> Building C command daemon..."
(cd command-daemon && cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build)
echo "    OK"

echo "==> Building Go api-gateway..."
(cd api-gateway && go build -o api-gateway .)
echo "    OK"

echo "==> Building TypeScript CLI..."
npm run build
echo "    OK"

echo "==> Build complete"
