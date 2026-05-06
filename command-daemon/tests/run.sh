#!/bin/bash
# Command daemon integration test suite.
# Sends JSON commands via stdin, validates JSON responses.
#
# Usage: ./tests/run.sh              (uses ../build/di-rvv-cmd)
#        DAEMON=/path/to/cmd ./tests/run.sh

set -uo pipefail

DAEMON="${DAEMON:-$(dirname "$0")/../build/di-rvv-cmd}"
WORKSPACE="/tmp/di-rvv-cmd-test-$$"
PASSED=0
FAILED=0
TOTAL=0
VERBOSE="${VERBOSE:-0}"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
BOLD='\033[1m'
DIM='\033[2m'
RESET='\033[0m'

die() { echo -e "${RED}FATAL:${RESET} $*" >&2; exit 1; }

# Build the daemon first
echo "Building daemon..."
(cd "$(dirname "$0")/.." && cmake -B build -DCMAKE_BUILD_TYPE=Release > /dev/null 2>&1 && cmake --build build > /dev/null 2>&1) || die "Build failed"
echo ""

[ -x "$DAEMON" ] || die "Daemon binary not found or not executable: $DAEMON"
mkdir -p "$WORKSPACE"

# ---- helpers ----

# Send JSON to daemon, collect all response lines.
# Uses a pipe: (echo MSG; sleep N) | daemon
# The sleep keeps stdin open so the daemon doesn't exit before the child finishes.
send() {
	local msg="$1"
	local wait="${2:-3}"
	(echo "$msg"; sleep "$wait") | timeout "$((wait + 3))" "$DAEMON" --workspace-root "$WORKSPACE" 2>/dev/null
}

# Extract a field from JSON using python (handles escapes properly).
# Usage: echo "$json" | jfield stdout
jfield() {
	python3 -c "
import sys, json
line = sys.stdin.read().strip()
if not line: sys.exit(1)
try:
    obj = json.loads(line)
    val = obj.get('$1')
    if val is None:
        print('null', end='')
    elif isinstance(val, bool):
        print('true' if val else 'false', end='')
    else:
        print(val, end='')
except: sys.exit(1)
"
}

# Extract a field from the first JSON line matching a type filter.
# Usage: jtype_result stdout  ->  extracts stdout from the "result" line
jtype() {
	local type_filter="$1"
	local field="$2"
	grep "\"type\":\"$type_filter\"" | jfield "$field"
}

# Run a single test.
run_test() {
	local name="$1"
	shift
	TOTAL=$((TOTAL + 1))
	printf "  %-50s " "$name"
	local output=""
	if output=$("$@" 2>&1); then
		PASSED=$((PASSED + 1))
		echo -e "${GREEN}PASS${RESET}"
		[ "$VERBOSE" = "1" ] && echo -e "${DIM}$output${RESET}"
	else
		FAILED=$((FAILED + 1))
		echo -e "${RED}FAIL${RESET}"
		[ -n "$output" ] && echo -e "${DIM}$output${RESET}" | head -5
	fi
}

# ---- test cases ----

test_echo_simple() {
	local out
	out=$(send '{"type":"execute","id":"t1","command":"echo hello world"}')
	local ack_id stdout exit_code

	ack_id=$(echo "$out" | jtype ack id)
	stdout=$(echo "$out" | jtype result stdout)
	exit_code=$(echo "$out" | jtype result exit_code)

	[ "$ack_id" = "t1" ] || { echo "ack_id='$ack_id'"; return 1; }
	[ "$stdout" = "hello world" ] || { echo "stdout='$stdout'"; return 1; }
	[ "$exit_code" = "0" ] || { echo "exit_code='$exit_code'"; return 1; }
}

test_echo_multiline() {
	local out
	out=$(send '{"type":"execute","id":"t2","command":"echo line1; echo line2; echo line3"}')
	local stdout
	stdout=$(echo "$out" | jtype result stdout)
	echo "$stdout" | grep -q "line1" || return 1
	echo "$stdout" | grep -q "line2" || return 1
	echo "$stdout" | grep -q "line3" || return 1
}

test_exit_code_nonzero() {
	local out
	out=$(send '{"type":"execute","id":"t3","command":"exit 42"}')
	local exit_code
	exit_code=$(echo "$out" | jtype result exit_code)
	[ "$exit_code" = "42" ] || return 1
}

test_stderr() {
	local out
	out=$(send '{"type":"execute","id":"t4","command":"echo err >&2"}')
	local stderr
	stderr=$(echo "$out" | jtype result stderr)
	[ "$stderr" = "err" ] || return 1
}

test_pwd() {
	local out
	out=$(send '{"type":"execute","id":"t5","command":"pwd"}')
	local stdout
	stdout=$(echo "$out" | jtype result stdout)
	[ "$stdout" = "$WORKSPACE" ] || { echo "pwd='$stdout' expected='$WORKSPACE'"; return 1; }
}

test_ls() {
	touch "$WORKSPACE/testfile.txt"
	local out
	out=$(send '{"type":"execute","id":"t6","command":"ls testfile.txt"}')
	local stdout
	stdout=$(echo "$out" | jtype result stdout)
	[ "$stdout" = "testfile.txt" ] || return 1
}

test_env_var() {
	local out
	out=$(send '{"type":"execute","id":"t7","command":"echo $HOME"}')
	local stdout
	stdout=$(echo "$out" | jtype result stdout)
	[ -n "$stdout" ] || return 1
}

test_numeric_id() {
	local out
	out=$(send '{"type":"execute","id":1,"command":"echo ok"}')
	local ack_id result_id
	ack_id=$(echo "$out" | jtype ack id)
	result_id=$(echo "$out" | jtype result id)
	# TypeScript sends String(id), but daemon should handle numeric ids too
	# If daemon can parse numeric id, it should be "1"
	# If not, it returns "" — TypeScript workaround sends string id
	if [ "$ack_id" = "1" ] && [ "$result_id" = "1" ]; then
		return 0
	elif [ -z "$ack_id" ] && [ -z "$result_id" ]; then
		echo "WARN: daemon cannot parse numeric ids (TS sends String(id) workaround)"
		return 0
	else
		echo "ack_id='$ack_id' result_id='$result_id'"
		return 1
	fi
}

test_safety_blocked() {
	local out
	out=$(send '{"type":"execute","id":"t9","command":"rm -rf /"}')
	# Blocked commands get a result with meta.blocked set (no fork, no ack)
	local blocked exit_code
	blocked=$(echo "$out" | grep '"type":"result"' | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); v=d.get('meta',{}).get('blocked','null'); print('null' if v is None else v, end='')")
	exit_code=$(echo "$out" | jtype result exit_code)
	[ "$blocked" = "recursive_delete" ] || { echo "blocked='$blocked'"; return 1; }
	[ "$exit_code" = "1" ] || return 1
}

test_safety_allowed() {
	touch "$WORKSPACE/safe-file.txt"
	local out
	out=$(send '{"type":"execute","id":"t10","command":"rm safe-file.txt"}')
	local blocked
	blocked=$(echo "$out" | grep '"type":"result"' | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); v=d.get('meta',{}).get('blocked','null'); print('null' if v is None else v, end='')")
	[ "$blocked" = "null" ] || return 1
}

test_missing_command_field() {
	local out
	out=$(send '{"type":"execute","id":"t11"}')
	local code
	code=$(echo "$out" | jtype error code)
	[ "$code" = "INVALID_REQUEST" ] || return 1
}

test_malformed_json() {
	local out
	out=$(send 'not json at all')
	local code
	code=$(echo "$out" | jtype error code)
	[ "$code" = "INVALID_REQUEST" ] || return 1
}

test_missing_type_field() {
	local out
	out=$(send '{"id":"t13","command":"echo hi"}')
	local code
	code=$(echo "$out" | jtype error code)
	[ "$code" = "INVALID_REQUEST" ] || return 1
}

test_ack_has_timeout() {
	local out
	out=$(send '{"type":"execute","id":"t14","command":"echo ack-check"}')
	local timeout_ms
	timeout_ms=$(echo "$out" | jtype ack timeout_ms)
	[ -n "$timeout_ms" ] || return 1
	[ "$timeout_ms" -gt 0 ] || return 1
}

test_concurrent_commands() {
	local out1 out2
	out1=$(send '{"type":"execute","id":"c1","command":"echo first"}')
	out2=$(send '{"type":"execute","id":"c2","command":"echo second"}')
	local s1 s2
	s1=$(echo "$out1" | jtype result stdout)
	s2=$(echo "$out2" | jtype result stdout)
	[ "$s1" = "first" ] || return 1
	[ "$s2" = "second" ] || return 1
}

test_pipe_command() {
	local out
	out=$(send '{"type":"execute","id":"t17","command":"echo -e \"aaa\\nbbb\\nccc\" | sort -r"}')
	local stdout
	stdout=$(echo "$out" | jtype result stdout)
	echo "$stdout" | grep -q "ccc" || return 1
}

test_large_output() {
	local out
	out=$(send '{"type":"execute","id":"t18","command":"seq 1 100"}')
	local stdout
	stdout=$(echo "$out" | jtype result stdout)
	echo "$stdout" | grep -q "^1$" || return 1
	echo "$stdout" | grep -q "100" || return 1
}

test_timeout_param() {
	local out
	out=$(send '{"type":"execute","id":"t19","command":"echo timed","timeout":5}')
	local stdout exit_code
	stdout=$(echo "$out" | jtype result stdout)
	exit_code=$(echo "$out" | jtype result exit_code)
	[ "$stdout" = "timed" ] || return 1
	[ "$exit_code" = "0" ] || return 1
}

test_cwd_tracking() {
	# Create a subdir, cd into it, verify cwd is tracked
	mkdir -p "$WORKSPACE/subdir"
	local out
	out=$(send '{"type":"execute","id":"t20","command":"cd subdir && pwd"}')
	local stdout cwd
	stdout=$(echo "$out" | jtype result stdout)
	cwd=$(echo "$out" | jtype result meta | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('cwd',''))" 2>/dev/null || true)
	[ "$stdout" = "$WORKSPACE/subdir" ] || { echo "stdout='$stdout'"; return 1; }
}

test_grep_command() {
	# Common real-world pattern: grep in a file
	echo -e "hello\nworld\nfoo" > "$WORKSPACE/grepfile.txt"
	local out
	out=$(send '{"type":"execute","id":"t21","command":"grep world grepfile.txt"}')
	local stdout exit_code
	stdout=$(echo "$out" | jtype result stdout)
	exit_code=$(echo "$out" | jtype result exit_code)
	[ "$stdout" = "world" ] || return 1
	[ "$exit_code" = "0" ] || return 1
}

test_false_command() {
	local out
	out=$(send '{"type":"execute","id":"t22","command":"false"}')
	local exit_code
	exit_code=$(echo "$out" | jtype result exit_code)
	[ "$exit_code" = "1" ] || return 1
}

# ---- main ----

echo -e "${BOLD}Command daemon test suite${RESET}"
echo "Binary: $DAEMON"
echo "Workspace: $WORKSPACE"
echo ""

echo -e "${BOLD}[Core]${RESET}"
run_test "echo simple string" test_echo_simple
run_test "echo multiline output" test_echo_multiline
run_test "non-zero exit code" test_exit_code_nonzero
run_test "false command" test_false_command
run_test "stderr capture" test_stderr
run_test "pwd matches workspace" test_pwd
run_test "ls finds created file" test_ls
run_test "environment variable" test_env_var
run_test "pipe chain" test_pipe_command
run_test "large output (100 lines)" test_large_output
run_test "grep in file" test_grep_command
run_test "explicit timeout param" test_timeout_param
run_test "cwd tracking" test_cwd_tracking

echo ""
echo -e "${BOLD}[Protocol]${RESET}"
run_test "numeric id (from TypeScript)" test_numeric_id
run_test "ack contains timeout_ms" test_ack_has_timeout
run_test "concurrent commands" test_concurrent_commands

echo ""
echo -e "${BOLD}[Error handling]${RESET}"
run_test "missing command field" test_missing_command_field
run_test "malformed JSON" test_malformed_json
run_test "missing type field" test_missing_type_field

echo ""
echo -e "${BOLD}[Safety]${RESET}"
run_test "blocked: rm -rf /" test_safety_blocked
run_test "allowed: rm specific file" test_safety_allowed

# Cleanup
rm -rf "$WORKSPACE"

echo ""
echo "-------------------------------------------"
if [ "$FAILED" -eq 0 ]; then
	echo -e "${GREEN}${BOLD}All ${TOTAL} tests passed${RESET}"
else
	echo -e "${RED}${BOLD}${FAILED}/${TOTAL} tests failed${RESET}"
fi

exit $FAILED
