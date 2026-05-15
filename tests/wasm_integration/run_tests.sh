#!/bin/bash
set -e

# Configuration
RUNNER="./bin/wasm-runner"
PYTHON_WASM="./standalone/runtime-files/python.wasm"
TEST_DIR="tests/wasm_integration"

echo "🚀 Starting Wasm-Python Integration Tests..."

# 1. Basic Execution
echo -n "Test 1: Basic Execution... "
OUTPUT=$($RUNNER --wasm "$PYTHON_WASM" --preopen "/lib:standalone/runtime-files/usr/local/lib" -- "$TEST_DIR/hello.py")
if [[ "$OUTPUT" == "Hello from WASM!" ]]; then
    echo "✅ PASS"
else
    echo "❌ FAIL (Got: '$OUTPUT')"
    exit 1
fi

# 2. Stdlib & Environment
echo -n "Test 2: Standard Library & CWD... "
OUTPUT=$($RUNNER --wasm "$PYTHON_WASM" --preopen "/lib:standalone/runtime-files/usr/local/lib" -- "$TEST_DIR/stdlib_test.py")
if echo "$OUTPUT" | grep -q '"status": "ok"'; then
    echo "✅ PASS"
else
    echo "❌ FAIL (Invalid JSON or missing status)"
    echo "Output: $OUTPUT"
    exit 1
fi

# 3. File I/O (Batch Edit Simulation)
echo -n "Test 3: File I/O (Read/Write)... "
echo "hello world" > "input.txt"
$RUNNER --wasm "$PYTHON_WASM" --preopen "/lib:standalone/runtime-files/usr/local/lib" -- "$TEST_DIR/file_io.py" > /dev/null

if [[ -f "output.txt" ]]; then
    RESULT=$(cat output.txt)
    if [[ "$RESULT" == "HELLO WORLD" ]]; then
        echo "✅ PASS"
    else
        echo "❌ FAIL (Content mismatch: '$RESULT')"
        exit 1
    fi
    rm output.txt
else
    echo "❌ FAIL (output.txt not created)"
    exit 1
fi
rm "input.txt"

# 4. Error Transparency (Tracebacks)
echo -n "Test 4: Error Transparency... "
echo "1 / 0" > "$TEST_DIR/error.py"
# Capture stderr
ERR_OUTPUT=$($RUNNER --wasm "$PYTHON_WASM" --preopen "/lib:standalone/runtime-files/usr/local/lib" -- "$TEST_DIR/error.py" 2>&1 || true)
if echo "$ERR_OUTPUT" | grep -q "ZeroDivisionError"; then
    echo "✅ PASS"
else
    echo "❌ FAIL (Traceback not captured)"
    echo "Output: $ERR_OUTPUT"
    exit 1
fi
rm "$TEST_DIR/error.py"

# 5. JSON Mode (IPC Protocol)
echo -n "Test 5: JSON IPC Mode... "
JSON_REQ="{\"wasm_path\":\"$PYTHON_WASM\",\"args\":[\"$TEST_DIR/hello.py\"],\"env\":[],\"preopens\":[[\"/lib\",\"standalone/runtime-files/usr/local/lib\"]]}"
JSON_RESP=$(echo "$JSON_REQ" | $RUNNER --json)
if echo "$JSON_RESP" | grep -q '"stdout":"Hello from WASM!\\n"'; then
    echo "✅ PASS"
else
    echo "❌ FAIL (JSON response mismatch)"
    echo "Response: $JSON_RESP"
    exit 1
fi

# 6. Environment & Arguments (JSON Mode)
echo -n "Test 6: Env & Args (JSON Mode)... "
JSON_REQ="{\"wasm_path\":\"$PYTHON_WASM\",\"args\":[\"$TEST_DIR/env_args.py\", \"HELLO_GUEST\"],\"env\":[[\"TEST_VAR\",\"WORLD\"]],\"preopens\":[[\"/lib\",\"standalone/runtime-files/usr/local/lib\"]]}"
JSON_RESP=$(echo "$JSON_REQ" | $RUNNER --json)
if echo "$JSON_RESP" | grep -q 'ENV_VAR: WORLD' && echo "$JSON_RESP" | grep -q 'GUEST_ARG: HELLO_GUEST'; then
    echo "✅ PASS"
else
    echo "❌ FAIL (Env or Args not passed correctly)"
    echo "Response: $JSON_RESP"
    exit 1
fi

# 7. Large Output (Buffering & Pressure)
echo -n "Test 7: Large Output (1MB each)... "
JSON_REQ="{\"wasm_path\":\"$PYTHON_WASM\",\"args\":[\"$TEST_DIR/large_output.py\"],\"env\":[],\"preopens\":[[\"/lib\",\"standalone/runtime-files/usr/local/lib\"]]}"
JSON_RESP=$(echo "$JSON_REQ" | $RUNNER --json)
STDOUT_LEN=$(echo "$JSON_RESP" | jq -r '.stdout | length' 2>/dev/null || echo "0")
STDERR_LEN=$(echo "$JSON_RESP" | jq -r '.stderr | length' 2>/dev/null || echo "0")

if [[ "$STDOUT_LEN" == "1048576" ]] && [[ "$STDERR_LEN" == "1048576" ]]; then
    echo "✅ PASS"
else
    echo "❌ FAIL (Output truncated: stdout=$STDOUT_LEN, stderr=$STDERR_LEN)"
    # echo "Response: $JSON_RESP"
    exit 1
fi

echo -e "\n🎉 All Wasm-Python Integration Tests Passed!"
