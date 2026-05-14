# Round 5 Audit — central-daemon (trie.c + main.c)

**Date:** 2026-05-14
**Auditors:** 5 parallel subagents (Memory Safety, Concurrency+Epoll, Performance+Allocations, Error Handling, Protocol Correctness)

---

## P0 — Must Fix (crash / memory corruption / data loss)

| ID | File | Line | Description | Status |
|----|------|------|-------------|--------|
| P0-1 | main.c | 763 | `fd_to_slot[client_fd] = slot` with no bounds check — client_fd could exceed MAX_EVENTS, causing OOB heap write | **FIXED** |
| P0-2 | main.c | 571 | `read()` returns -1 with EINTR → treated as fatal, triggers client cleanup | **FIXED** |
| P0-3 | trie.c | 531 | `fclose` return not checked — if it fails, unlink proceeds on empty/corrupt tmp file | **FIXED** |
| P0-4 | trie.c | 136 | `ht_remove` in `node_prune_upward` unchecked — stale entry if remove fails, UAF on next lookup | **FIXED** |
| P0-5 | trie.c | 332,341,366 | `ht_remove` return values ignored in trie_set_config (transient + persistent paths) | **FIXED** |
| P0-6 | trie.c | 292,401 | `strdup` return unchecked in `trie_traverse` and `trie_get_config` — NULL → segfault | **FIXED** |
| P0-7 | trie.c | 806,827 | `malloc` return unchecked in `trie_cleanup_fd` wakeup paths — NULL → crash | **FIXED** |
| P0-8 | trie.c | 605-609 | `realloc` failure in `register_node_to_fd` leaves waiters array inconsistent (cap halved, stale fd remains) | **FIXED** |
| P0-9 | main.c | 89 | `epoll_ctl` MOD failure in partial send → EPOLLOUT never re-registered, outbuf stalls | **FIXED** |
| P0-10 | main.c | 121 | `epoll_ctl` MOD to unregister EPOLLOUT unchecked → spinning on closed fd if DEL fails | **FIXED** |

---

## P1 — Significant (logic bug / resource leak / performance)

| ID | File | Line | Description | Status |
|----|------|------|-------------|--------|
| P1-1 | trie.h / trie.c | 43-45 | `trie_get_stats` reads non-atomic `size_t` counters from signal handler while event loop mutates | **FIXED** — made counters `_Atomic size_t` |
| P1-2 | main.c | 706-711 | `trie_save_persist` called from signal handler context (deferred flag) — not async-signal-safe | **WONTFIX** — flag-only signal handler, I/O deferred to main loop |
| P1-3 | main.c | 59,127 | Unregister EPOLLOUT sets `out_epoll_registered=false` BEFORE epoll_ctl succeeds | **FIXED** — flag set only after confirmed MOD succeeds |
| P1-4 | trie.c | 649 | `realloc(current->waiters, +1)` per waiter — heap churn under contention | **FIXED** — doubling growth (initial 32, then cap*2), graceful OOM with consistent state |
| P1-5 | trie.c | 292,417 | `strdup(path)` in `trie_traverse` and `trie_get_config` — redundant copy on every acquire/release/set_config | **FIXED** — replaced with inline segment parsing (zero malloc per call) |
| P1-6 | trie.c | 327-328 | HT `initial_capacity=8` for transient_registry per-FD tables — rehash storm for chatty clients | **FIXED** — bumped to 16 |
| P1-7 | main.c | 701 | `setsockopt(SO_REUSEADDR)` return ignored — startup fails silently if it doesn't bind | **FIXED** |
| P1-8 | main.c | 705 | `unlink` before bind return ignored — bind failure misattributed | **FIXED** |
| P1-9 | trie.c | 724-755 | `send_granted_cb` calls `epoll_ctl` MOD inside epoll dispatch loop — re-entrancy risk | **WONTFIX** — single-threaded event loop prevents re-entrancy; documented in trie.c header |
| P1-10 | main.c | 563 | `find_string_val` silently truncates raw strings >8191 bytes into 8192-byte buffer | **FIXED** — dynamic malloc+free raw buffer, no hard limit |

---

## P2 — Minor (observability / edge cases / SBC concerns)

| ID | File | Line | Description | Status |
|----|------|------|-------------|--------|
| P2-1 | main.c | 226 | Broadcast send_json failures only logged, not acted on — client misses config update | TODO |
| P2-2 | main.c | 418 | `trie_set_config` returns -1 for all failure modes — no per-error-context | TODO |
| P2-3 | main.c | 59 | `epoll_ctl` MOD partial send initial registration has no error check | **FIXED** (incorporated into P0-9/P1-3 fix) |
| P2-4 | trie.c | 547-548 | `errno` post-fclose conflated between fopen failure and parse failure | TODO |
| P2-5 | main.c | 251 | `write(STDERR_FILENO)` return explicitly cast to void in `handle_stats` | TODO |
| P2-6 | trie.c | 506 | `node_save_recursive` uses ~17KB stack per frame × 256 depth = ~4.3MB stack — overflow on Pi 3B | **FIXED** — converted to iterative pre-order traversal |
| P2-7 | trie.c | 807 | `node_get_path` recursive with 4096-byte temp per level — depth 256 = 1MB stack | **FIXED** — converted to iterative with fixed-size segment array |
| P2-8 | trie.c | 411 | `trie_node_t *nodes[257]` VLA heap allocation per `get_config` call | TODO |
| P2-9 | main.c | 556-564 | `process_json_stream` else branch: `p = obj_start + 1` can re-visit same `{` on next call if new data appended | **FIXED** — always break and wait for more data when no closing brace found |

---

## Resolved in Prior Rounds (do not edit)

- Round 4: OOB in json_unescape, value=NULL leak, ht_table_t struct double-free, signal handler fprintf, EBADF guards on close, trie_set_config return checks, drain_output EINTR retry
- Round 3: EINTR on write, close return checked, EPOLLONESHOT, broadcast send failures logged
- Round 2: Transient OOM leak, empty segment guards, async-signal-safe stats, dynamic client buffers