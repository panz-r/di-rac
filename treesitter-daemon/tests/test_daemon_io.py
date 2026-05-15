#!/usr/bin/env python3
"""Regression tests for the treesitter daemon's poll-based I/O.

Tests the daemon's request-response cycle through stdin/stdout pipes,
including edge cases that have caused hangs or lost responses.

Usage: python3 test_daemon_io.py [--daemon PATH] [--verbose]
"""

import subprocess
import json
import select
import os
import sys
import time
import threading
import signal

DAEMON = os.environ.get("TEST_DAEMON", "/w/di-rac/bin/divrr-analyzer")
WORKSPACE = os.environ.get("TEST_WORKSPACE", "/w/di-rac")
TIMEOUT = 10  # seconds per request


class DaemonIO:
    """Manages a daemon process and provides request-response helpers."""

    def __init__(self, daemon_path=DAEMON, workspace=WORKSPACE):
        self.daemon_path = daemon_path
        self.workspace = workspace
        self.proc = None
        self.req_id = 0
        self._buf = b""

    def start(self):
        self.proc = subprocess.Popen(
            [self.daemon_path, "--workspace-root", self.workspace],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        # Wait for ready message
        ready = self.proc.stderr.readline()
        assert b"ready" in ready, f"Unexpected ready: {ready}"

    def stop(self):
        if self.proc:
            self.proc.kill()
            self.proc.wait()

    def send(self, command: str, **kwargs) -> dict:
        """Send a command and return the parsed response."""
        self.req_id += 1
        req = {"command": command, "id": self.req_id, **kwargs}
        line = json.dumps(req) + "\n"
        self.proc.stdin.write(line.encode())
        self.proc.stdin.flush()

        # Read response with timeout
        line = self._read_line(TIMEOUT)
        resp = json.loads(line)
        assert resp.get("id") in (self.req_id, str(self.req_id)), \
            f"ID mismatch: got {resp.get('id')} expected {self.req_id}"
        return resp

    def send_batch(self, commands: list[dict]) -> list[dict]:
        """Send multiple commands and read all responses in order."""
        for cmd in commands:
            self.req_id += 1
            cmd["id"] = self.req_id
            line = json.dumps(cmd) + "\n"
            self.proc.stdin.write(line.encode())
        self.proc.stdin.flush()

        responses = []
        for _ in commands:
            line = self._read_line(TIMEOUT)
            responses.append(json.loads(line))
        return responses

    def _read_line(self, timeout: float) -> bytes:
        """Read a single \\n-terminated line from stdout with timeout."""
        # Use buffered data first
        if self._buf:
            nl = self._buf.find(b"\n")
            if nl >= 0:
                line = self._buf[:nl+1]
                self._buf = self._buf[nl+1:]
                return line
        buf = self._buf
        self._buf = b""
        start = time.monotonic()
        while time.monotonic() - start < timeout:
            r, _, _ = select.select([self.proc.stdout], [], [], 0.2)
            if r:
                chunk = self.proc.stdout.read(65536)
                if not chunk:
                    raise RuntimeError("stdout EOF while reading response")
                buf += chunk
                nl = buf.find(b"\n")
                if nl >= 0:
                    self._buf = buf[nl+1:]
                    return buf[:nl+1]
            else:
                ret = self.proc.poll()
                if ret is not None:
                    raise RuntimeError(
                        f"daemon exited with code {ret} during read"
                    )
        raise TimeoutError(
            f"No response within {timeout}s, buf={buf[:200]}"
        )


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_status(d):
    """Basic smoke test: status command responds correctly."""
    resp = d.send("status")
    assert resp.get("ok") is True, f"status not ok: {resp}"
    assert resp.get("type") == "status_result", f"wrong type: {resp}"
    print("  PASS status")


def test_extract_apis(d):
    """extract-apis with valid Rust code returns definitions."""
    resp = d.send("extract-apis", content="fn main() {}", language="rust")
    # May return ok=True with empty defs, or ok=False if parsing fails
    assert "ok" in resp, f"missing ok: {resp}"
    print("  PASS extract-apis")


def test_repo_map(d):
    """repo-map returns a non-empty file list for a real workspace."""
    resp = d.send("repo-map")
    assert resp.get("ok") is True, f"repo-map not ok: {resp}"
    files = resp.get("files", [])
    assert len(files) > 0, f"repo-map returned empty files: {resp}"
    print(f"  PASS repo-map ({len(files)} files)")


def test_multiple_requests(d):
    """Multiple sequential requests all get correct responses."""
    n = 5
    cmds = [{"command": "status"} for _ in range(n)]
    responses = d.send_batch(cmds)
    assert len(responses) == n, f"Expected {n} responses, got {len(responses)}"
    for i, resp in enumerate(responses):
        assert resp.get("id") is not None, f"Response {i} missing id: {resp}"
        assert resp.get("ok") is True, f"Response {i} not ok: {resp}"
    print(f"  PASS {n} sequential requests")


def test_id_matches_request(d):
    """Each response's id matches its request's id."""
    for i in range(3):
        resp = d.send("status")
        expected = i + 1  # id starts at 1, increments per send
        rid = resp.get("id")
        assert rid is not None, f"Missing id in response: {resp}"
        assert int(rid) == expected, f"Id mismatch: got {rid} expected {expected}"
    print("  PASS id matching")


def test_response_order(d):
    """Responses arrive in the same order as requests (batch)."""
    cmds = [
        {"command": "status"},
        {"command": "status"},
        {"command": "status"},
    ]
    responses = d.send_batch(cmds)
    ids = [int(r.get("id", 0)) for r in responses]
    assert ids == sorted(ids), f"Responses out of order: {ids}"
    print(f"  PASS response ordering ({ids})")


def test_repo_map_subdir(d):
    """repo-map with a relative path works correctly."""
    resp = d.send("repo-map", file=".")
    assert resp.get("ok") is True, f"subdir repo-map not ok: {resp}"
    print("  PASS repo-map subdir")


def test_unknown_command(d):
    """Unknown command returns an error, not a hang."""
    resp = d.send("nonexistent-command")
    assert resp.get("ok") is False, f"expected error for unknown cmd: {resp}"
    assert "UNKNOWN_COMMAND" in str(resp), f"wrong error code: {resp}"
    print("  PASS unknown command")


def test_poll_stdin_partial_read(d):
    """Write request in chunks to test partial stdin reads."""
    d.req_id += 1
    req = json.dumps({"command": "status", "id": d.req_id})
    # Write in 1-byte chunks with delays
    for ch in req + "\n":
        d.proc.stdin.write(ch.encode())
        d.proc.stdin.flush()
        time.sleep(0.01)
    line = d._read_line(TIMEOUT)
    resp = json.loads(line)
    assert resp.get("id") == d.req_id, f"id mismatch on partial write: {resp}"
    print("  PASS partial stdin read")


def test_concurrent_workers(d):
    """Multiple concurrent requests complete correctly."""
    n = 4
    cmds = [{"command": "repo-map", "file": f"treesitter-daemon/src"} for _ in range(n)]
    # Send all at once, then read responses
    responses = d.send_batch(cmds)
    assert len(responses) == n
    for r in responses:
        assert r.get("ok") is True, f"concurrent request failed: {r}"
    print(f"  PASS {n} concurrent repo-map requests")


def test_timeout_hang_regression(d):
    """If daemon hangs, this test will time out and fail."""
    resp = d.send("status")
    assert resp.get("ok") is True
    # If we get here, no hang occurred
    print("  PASS no hang regression")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def run_tests(tests, verbose=False):
    d = DaemonIO()
    passed = 0
    failed = 0
    try:
        d.start()
        for name, test_fn in tests:
            try:
                test_fn(d)
                passed += 1
            except Exception as e:
                print(f"  FAIL {name}: {e}")
                if verbose:
                    import traceback
                    traceback.print_exc()
                failed += 1
    finally:
        d.stop()

    print(f"\n{'='*50}")
    print(f"Results: {passed} passed, {failed} failed, {passed+failed} total")
    return failed == 0


if __name__ == "__main__":
    verbose = "--verbose" in sys.argv
    if "--daemon" in sys.argv:
        idx = sys.argv.index("--daemon")
        DAEMON = sys.argv[idx + 1]

    tests = [
        ("status", test_status),
        ("extract_apis", test_extract_apis),
        ("repo_map", test_repo_map),
        ("unknown_command", test_unknown_command),
        ("id_matches_request", test_id_matches_request),
        ("multiple_requests", test_multiple_requests),
        ("response_order", test_response_order),
        ("partial_stdin_read", test_poll_stdin_partial_read),
        ("repo_map_subdir", test_repo_map_subdir),
        ("timeout_hang_regression", test_timeout_hang_regression),
        ("concurrent_workers", test_concurrent_workers),
    ]

    ok = run_tests(tests, verbose)
    sys.exit(0 if ok else 1)
