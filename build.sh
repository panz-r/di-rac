#!/bin/bash
set -e

cd "$(dirname "$0")"

echo "==> Building Go api-gateway..."
(cd api-gateway && go build -o api-gateway .)
echo "    OK"

echo "==> Building TypeScript CLI..."
npm run build
echo "    OK"

echo "==> Build complete"
