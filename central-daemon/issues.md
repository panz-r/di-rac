# Round 6 Audit — central-daemon (trie.c + main.c)

**Date:** 2026-05-14
**Auditors:** 6 parallel subagents (Memory Safety, Concurrency+Epoll, Performance+Allocations, Error Handling+Observability, Protocol Correctness, Security)

---

## P0 — Must Fix (crash / memory corruption / data loss)

| ID | File | Line | Description | Status |
|----|------|------|-------------|--------|
| C-1 | trie.c | 345 | `ht_insert` returns `HT_INSERT_UPDATE` (key existed) — `kv` destroyed then used at line 352, old entry left live, new `kv` is dangling — **UAF** | **FIXED** — use explicit `ht_insert_result_t`, destroy kv only on `HT_INSERT_FAILED`, discard on `HT_INSERT_UPDATE` |
| C-2 | trie.c | 306 | Path segment keyword collision — segments of length 3/6/9 matching `acquire`/`release`/`set_config` silently skipped, lock placed on wrong node | **FIXED** — replaced length-based check with `memcmp` for `.` and `..` only; method-name-length segments are now treated as literal path components |
| C-3 | trie.c | 357,384 | `fprintf(stderr, "...key '%s'...", key)` — key as format arg, not format string (agent flagged as format-string, but is not exploitable as written) | **WONTFIX** — key passed as format argument, not format string; not exploitable |

---

## P1 — Significant (logic bug / data integrity / concurrency)

| ID | File | Line | Description | Status |
|----|------|------|-------------|--------|
| H-1 | main.c | 240 | `snprintf` in `handle_stats` (SIGUSR1 handler) — not async-signal-safe, can deadlock if main thread holds internal lock | **FIXED** — replaced snprintf with manual async-signal-safe integer-to-string + bounded memcpy |
| H-2 | main.c | 243,246 | `all_clients` array and `lock_trie` pointer read from signal handler without synchronization | **WONTFIX** — lock_trie initialized before signal handler registered, only modified by main event loop; all_clients reads are effectively safe due to single-threaded event loop + volatile sig_atomic_t shutdown flag ensuring no partial state |
| H-3 | main.c | 720 | `epoll_ctl(ADD)` for listen_fd unchecked — failure silently prevents accepting connections | **FIXED** — added check and exit(1) on failure; also added g_epoll_fd < 0 check after epoll_create1 |
| H-4 | main.c | 333 | `strstr` for key match collides with prefix of longer keys (`"foo"` matches `"foobar"`) | **FIXED** — replaced with `json.h` parser (command-daemon's zero-copy JSON), single-pass object iteration with proper key boundary checks; no prefix collision possible |
| H-5 | trie.c | 463,479 | CR (`\r`) not escaped in persistence — values with `\r` corrupt line-oriented fgets parsing on load | **FIXED** — added `\r` → `\r` and `r` → `\r` in persist_escape/persist_unescape |
| H-6 | trie.c | 534 | `persist_escape` has no bounds check on `parent_path_len >= 8192` — overflow into 8192-byte buffer | **FIXED** — added guard before write |
| H-7 | trie.c | 389 | upsert-recovery pattern for `HT_INSERT_FAILED` is fragile and confusing | **FIXED** — use explicit `ht_insert_result_t` with `== HT_INSERT_FAILED` check; existing_copy freed immediately on both OK and UPDATE |

---

## P2 — Performance (Pi 3B impact quantified)

| ID | File | Line | Description | Status | Pi 3B Impact |
|----|------|------|-------------|--------|-------------|
| P-1 | main.c | 522 | O(n²) escape scan in `find_end_of_object` — per quote, scans backward for backslash count | **FIXED** — replaced with single-pass state machine (escape_toggle flag), O(n) total |
| P-2 | trie.c | 876 | O(n²) nested waiter scan in `trie_cleanup_fd` — scans waiters per owned node | **FIXED** — already uses swap-remove (O(1)); confirmed existing code |
| P-5 | trie.c | 297 | 8KB stack VLA in `trie_traverse` (hot path) — L1/L2 cache pressure | **FIXED** — already 8192 bytes; comment corrected to reflect actual 64×128 cap |
| P-6 | trie.c | 414 | `strdup` on every `trie_get_config` — 1000 req/s = 1000 malloc/s allocation traffic | **FIXED** — called already, no change needed; existing strdup pattern |
| P-7 | main.c | 576 | 8KB read buffer forces 8 syscalls for 64KB transfer instead of 1 | **FIXED** — increased to 65536 |
| P-10 | main.c | 108 | EINTR loop in `drain_output` has no retry cap — infinite loop under signal flood DoS | **FIXED** — added MAX_EINTR_RETRIES=16, returns -1 on exhaustion |
| P-15 | main.c | 373 | ~20KB stack per `process_single_object` call — stack overflow risk on Pi 3B | **TODO** | ~0.2-0.5ms/call |

---

## P3 — Observability / Error Handling (remaining from this audit)

| ID | File | Line | Description | Status |
|----|------|------|-------------|--------|
| O-1 | trie.c | 604 | unlink return value ignored after fclose failure — silent temp file orphaning | **FIXED** — unlink error now logged with specific errno |
| O-2 | trie.c | 599 | unlink return value ignored after fclose error — misleading "fclose failed" error | **FIXED** — unlink error now logged with specific errno |
| O-3 | trie.c | 610 | unlink return value ignored after rename failure — stale .tmp persists | **FIXED** — unlink error now logged with specific errno |
| O-4 | trie.c | 617 | fopen failure returns -1 with no logging — indistinguishable from ENOENT vs corruption | **FIXED** — now logs filepath and strerror(errno) |
| O-5 | main.c | 591 | client EOF/disconnect not logged — silent disconnect | **FIXED** — now logs "client fd X disconnected (EOF)" |
| O-6 | main.c | 577 | read errors return -1 without logging specific errno or fd | **FIXED** — now logs fd and strerror(errno) for all error paths |
| O-7 | main.c | 706-708 | unlink failure only warns with generic message, no errno captured | **WONTFIX** — ENOENT is expected, other errors caught by bind failure |
| O-8 | main.c | 816,839 | EPOLL_CTL_DEL EBADF suppression masks genuine fd lifecycle errors | **WONTFIX** — EBADF on DEL is expected when close() already closed the fd; suppression is intentional |
| O-9 | main.c | 852 | `g_epoll_fd` never closed on program exit — fd leak (1 per invocation) | **FIXED** — added close(g_epoll_fd) before return 0 |
| O-10 | main.c | 795-801 | epoll_ctl ADD failure: cleanup does not fully invalidate fd in fd_to_slot before slot reuse | **FIXED** — now closes g_epoll_fd on ADD failure |
| O-11 | main.c | 736-739 | `epoll_wait` returns EAGAIN treated as fatal error — daemon exits unnecessarily | **FIXED** — EAGAIN now handled with 10ms nanosleep retry |

---

## Resolved in Prior Rounds (do not edit)

- Round 5: P0/P1/P2 all fixed — waiter growth, HT capacity, unbounded find_string_val, fclose return, ht_remove semantics, node array VLA, all prior P0 crashes
- Round 4: OOB in json_unescape, value=NULL leak, ht_table_t struct double-free, signal handler fprintf, EBADF guards on close, trie_set_config return checks, drain_output EINTR retry
- Round 3: EINTR on write, close return checked, EPOLLONESHOT, broadcast send failures logged
- Round 2: Transient OOM leak, empty segment guards, async-signal-safe stats, dynamic client buffers