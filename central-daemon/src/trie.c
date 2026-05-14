/* Thread-safety model:
 * The event loop runs on a single thread. All trie operations are serialized
 * by the epoll_wait loop — no concurrent access to trie state occurs during
 * normal flow. Signal handlers (e.g., SIGUSR1 stats) can interrupt the main
 * thread and read trie state; ensure all such reads are async-signal-safe.
 * If multi-threaded access is added later, a pthread_mutex must protect all
 * trie operations and the stat counters must be made atomic or mutex-guarded.
 *
 * Locking hierarchy: node-level locks via owner_fd + intent_count, plus
 * per-FD registries (fd_registry, waiting_registry, transient_registry).
 * Lock acquisition order: always lock child before parent to avoid deadlock.
 * Cleanup order: waiting_registry before fd_registry (see trie_cleanup_fd).
 */

#include "trie.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <errno.h>
#include <unistd.h>

/* --- Registry Structure --- */

typedef struct {
    trie_node_t **nodes;
    size_t count;
    size_t cap;
} node_list_t;

/* --- Helpers for Draugr --- */

static uint64_t string_hash(const void *key, size_t key_len, void *user_ctx) {
    (void)user_ctx;
    const uint8_t *p = (const uint8_t *)key;
    uint64_t h = 0xcbf29ce484222325ULL;
    for (size_t i = 0; i < key_len; i++) {
        h ^= p[i];
        h *= 0x100000001b3ULL;
    }
    return h;
}

static bool string_eq(const void *key_a, size_t len_a,
                      const void *key_b, size_t len_b, void *user_ctx) {
    (void)user_ctx;
    if (len_a != len_b) return false;
    return memcmp(key_a, key_b, len_a) == 0;
}

static uint64_t fd_hash(const void *key, size_t key_len, void *user_ctx) {
    (void)user_ctx; (void)key_len;
    uint64_t h = *(const int*)key;
    h ^= h >> 16;
    h *= 0x85ebca6b;
    h ^= h >> 13;
    h *= 0xc2b2ae35;
    h ^= h >> 16;
    return h;
}

static bool fd_eq(const void *key_a, size_t len_a,
                  const void *key_b, size_t len_b, void *user_ctx) {
    (void)user_ctx; (void)len_a; (void)len_b;
    return *(const int*)key_a == *(const int*)key_b;
}

/* --- Core Node Implementation --- */

static trie_node_t* node_create(const char *segment, trie_node_t *parent) {
    trie_node_t *node = calloc(1, sizeof(trie_node_t));
    if (!node) return NULL;

    if (segment) {
        node->segment = strdup(segment);
        if (!node->segment) { free(node); return NULL; }
    }
    node->parent = parent;
    node->owner_fd = -1;

    ht_config_t cfg = {
        .initial_capacity = 16,
        .max_load_factor = 0.75,
        .min_load_factor = 0.20,
        .tomb_threshold = 0.20,
        .zombie_window = 8
    };
    node->children = ht_create(&cfg, string_hash, string_eq, NULL);
    if (!node->children) { free(node->segment); free(node); return NULL; }
    node->settings = ht_create(&cfg, string_hash, string_eq, NULL);
    if (!node->settings) { ht_destroy(node->children); free(node->segment); free(node); return NULL; }

    return node;
}

static void node_destroy(trie_node_t *node) {
    if (!node) return;
    
    ht_iter_t it = ht_iter_begin(node->children);
    const void *key, *val;
    size_t klen, vlen;
    while (ht_iter_next(node->children, &it, &key, &klen, &val, &vlen)) {
        trie_node_t *child = *(trie_node_t**)val;
        node_destroy(child);
    }
    
    ht_destroy(node->children);
    
    /* Clean up settings */
    it = ht_iter_begin(node->settings);
    while (ht_iter_next(node->settings, &it, &key, &klen, &val, &vlen)) {
        free(*(char**)val);
    }
    ht_destroy(node->settings);

    free(node->segment);
    free(node->waiters);
    free(node);
}

static bool node_is_empty(trie_node_t *node) {
    if (!node) return false;
    if (node->owner_fd != -1) return false;
    if (node->intent_count != 0) return false;
    if (node->waiters_count != 0) return false;
    if (ht_size(node->children) != 0) return false;
    if (ht_size(node->settings) != 0) return false;
    return true;
}

static void node_prune_upward(trie_t *trie, trie_node_t *node) {
    while (node && node->parent) {
        if (!node_is_empty(node)) break;
        if (!node->segment || !node->segment[0]) { node = node->parent; continue; }
        trie_node_t *parent = node->parent;
        size_t seg_len = strlen(node->segment);
        if (ht_remove(parent->children, node->segment, seg_len) == 0) {
            fprintf(stderr, "[di-vrr] node_prune_upward: ht_remove failed for segment '%s'\n",
                    node->segment ? node->segment : "(null)");
        }
        node_destroy(node);
        trie->total_nodes--;
        node = parent;
    }
}

trie_t* trie_create(void) {
    trie_t *trie = malloc(sizeof(trie_t));
    if (!trie) return NULL;
    trie->root = node_create(NULL, NULL);
    if (!trie->root) { free(trie); return NULL; }

    ht_config_t cfg = {
        .initial_capacity = 64,
        .max_load_factor = 0.75,
        .min_load_factor = 0.20,
        .tomb_threshold = 0.20,
        .zombie_window = 16
    };
    trie->fd_registry = ht_create(&cfg, fd_hash, fd_eq, NULL);
    trie->waiting_registry = ht_create(&cfg, fd_hash, fd_eq, NULL);
    trie->transient_registry = ht_create(&cfg, fd_hash, fd_eq, NULL);
    if (!trie->fd_registry || !trie->waiting_registry || !trie->transient_registry) {
        if (trie->fd_registry) ht_destroy(trie->fd_registry);
        if (trie->waiting_registry) ht_destroy(trie->waiting_registry);
        node_destroy(trie->root);
        free(trie);
        return NULL;
    }

    trie->total_nodes = 1;   /* root node created above */
    trie->total_locks = 0;
    trie->total_waiters = 0;

    return trie;
}

static void free_registry_contents(ht_table_t *registry) {
    size_t n = ht_size(registry);
    node_list_t **lists = malloc(sizeof(node_list_t*) * n);
    size_t count = 0;

    if (lists) {
        ht_iter_t it = ht_iter_begin(registry);
        const void *key, *val;
        size_t klen, vlen;
        while (ht_iter_next(registry, &it, &key, &klen, &val, &vlen)) {
            lists[count++] = *(node_list_t**)val;
        }
        ht_destroy(registry);
        for (size_t i = 0; i < count; i++) { free(lists[i]->nodes); free(lists[i]); }
        free(lists);
    } else {
        ht_iter_t it = ht_iter_begin(registry);
        const void *key, *val;
        size_t klen, vlen;
        while (ht_iter_next(registry, &it, &key, &klen, &val, &vlen)) {
            node_list_t *list = *(node_list_t**)val;
            free(list->nodes);
            free(list);
        }
        ht_destroy(registry);
    }
}

static void free_kv_table(ht_table_t *kv) {
    size_t n = ht_size(kv);
    char **vals = malloc(sizeof(char*) * n);
    size_t count = 0;

    if (vals) {
        ht_iter_t it = ht_iter_begin(kv);
        const void *key, *val;
        size_t klen, vlen;
        while (ht_iter_next(kv, &it, &key, &klen, &val, &vlen)) {
            vals[count++] = *(char**)val;
        }
        ht_destroy(kv);
        for (size_t i = 0; i < count; i++) free(vals[i]);
        free(vals);
    } else {
        ht_iter_t it = ht_iter_begin(kv);
        const void *key, *val;
        size_t klen, vlen;
        while (ht_iter_next(kv, &it, &key, &klen, &val, &vlen)) {
            free(*(char**)val);
        }
        ht_destroy(kv);
    }
}

void trie_destroy(trie_t *trie) {
    if (!trie) return;
    node_destroy(trie->root);
    free_registry_contents(trie->fd_registry);
    free_registry_contents(trie->waiting_registry);
    
    /* Clean up transient settings */
    size_t n_trans = ht_size(trie->transient_registry);
    ht_table_t **tables = malloc(sizeof(ht_table_t*) * n_trans);
    size_t count = 0;

    if (tables) {
        ht_iter_t it = ht_iter_begin(trie->transient_registry);
        const void *key, *val;
        size_t klen, vlen;
        while (ht_iter_next(trie->transient_registry, &it, &key, &klen, &val, &vlen)) {
            tables[count++] = *(ht_table_t**)val;
        }
        ht_destroy(trie->transient_registry);
        for (size_t i = 0; i < count; i++) {
            free_kv_table(tables[i]);
            free(tables[i]);
        }
        free(tables);
    } else {
        ht_iter_t it = ht_iter_begin(trie->transient_registry);
        const void *key, *val;
        size_t klen, vlen;
        while (ht_iter_next(trie->transient_registry, &it, &key, &klen, &val, &vlen)) {
            free_kv_table(*(ht_table_t**)val);
            free(*(ht_table_t**)val);
        }
        ht_destroy(trie->transient_registry);
    }
    
    free(trie);
}

/* --- Trie Helper Operations --- */

static trie_node_t* node_get_child(trie_node_t *node, const char *segment, bool create, trie_t *trie) {
    if (!segment || !segment[0]) return NULL;
    size_t segment_len = strlen(segment);
    size_t vlen;
    const void *found = ht_find(node->children, segment, segment_len, &vlen);

    if (found) return *(trie_node_t**)found;
    if (!create) return NULL;

    trie_node_t *new_node = node_create(segment, node);
    if (!new_node) return NULL;

    if (ht_insert(node->children, segment, segment_len, &new_node, sizeof(trie_node_t*)) == HT_INSERT_FAILED) {
        node_destroy(new_node);
        return NULL;
    }
    trie->total_nodes++;
    return new_node;
}

trie_node_t* trie_traverse(trie_t *trie, const char *path, bool create, bool *ancestor_locked) {
    if (!path) return NULL;
    /* Parse segments into a fixed-size array without strdup.
     * We use a small inline buffer to avoid malloc in the hot path.
     * Max 256 segments × ~128 bytes each = ~32KB worst-case on stack.
     * For normal paths (depth < 10) this is negligible. */
    char seg_buf[8192];
    size_t seg_count = 0;

    const char *p = path;
    while (*p == '/') p++;
    while (*p && seg_count < 64) {  /* 64 segments × 128 bytes = 8192 bytes total */
        const char *seg_start = p;
        while (*p && *p != '/') p++;
        size_t seg_len = (size_t)(p - seg_start);
        /* Use content comparison (memcmp), not length-based heuristics,
         * to avoid silent collision with method-name-length segments. */
        if (seg_len == 1 && memcmp(seg_start, ".", 1) == 0) {}
        else if (seg_len == 2 && memcmp(seg_start, "..", 2) == 0) { if (seg_count > 0) seg_count--; }
        else {
            if (seg_len > 127) seg_len = 127;  /* cap to fit in 128-byte slot */
            memcpy(seg_buf + seg_count * 128, seg_start, seg_len);
            seg_buf[seg_count * 128 + seg_len] = '\0';
            seg_count++;
        }
        while (*p == '/') p++;
    }

    trie_node_t *current = trie->root;
    for (size_t i = 0; i < seg_count; i++) {
        if (ancestor_locked && current->owner_fd != -1) {
            *ancestor_locked = true;
            return NULL;
        }
        const char *seg = seg_buf + i * 128;
        current = node_get_child(current, seg, create, trie);
        if (!current) return NULL;
    }
    return current;
}

/* --- Configuration Management --- */

int trie_set_config(trie_t *trie, const char *path, int fd, const char *key, const char *value, bool transient) {
    size_t klen = strlen(key);
    size_t ulen;

    if (transient) {
        ht_table_t *kv;
        const void *found = ht_find(trie->transient_registry, &fd, sizeof(int), &ulen);
        if (found) {
            kv = *(ht_table_t**)found;
        } else {
            ht_config_t cfg = {.initial_capacity = 16, .max_load_factor = 0.75, .min_load_factor = 0.20, .tomb_threshold = 0.20, .zombie_window = 8};
            kv = ht_create(&cfg, string_hash, string_eq, NULL);
            if (!kv) return -1;
            ht_insert_result_t ins = ht_insert(trie->transient_registry, &fd, sizeof(int), &kv, sizeof(ht_table_t*));
            if (ins == HT_INSERT_FAILED) {
                ht_destroy(kv);
                ht_remove(trie->transient_registry, &fd, sizeof(int));
                return -1;
            }
            /* HT_INSERT_UPDATE: key already existed, old entry untouched, kv is orphaned — discard it */
            if (ins == HT_INSERT_UPDATE) ht_destroy(kv);
        }
        
        const void *existing = ht_find(kv, key, klen, &ulen);
        char *existing_copy = NULL;
        if (existing) {
            existing_copy = *(char**)existing;
            if (ht_remove(kv, key, klen) == 0) {
                fprintf(stderr, "[di-vrr] trie_set_config: ht_remove failed for key '%s' (transient)\n", key);
            }
        }

        if (value) {
            char *v_copy = strdup(value);
            ht_insert_result_t ins_kv = ht_insert(kv, key, klen, &v_copy, sizeof(char*));
            if (ins_kv == HT_INSERT_FAILED) {
                free(v_copy);
                if (existing_copy) {
                    ht_insert(kv, key, klen, &existing_copy, sizeof(char*));
                }
                return -1;
            }
            /* HT_INSERT_UPDATE or HT_INSERT_OK: existing_copy is superseded */
            if (existing_copy) free(existing_copy);
        } else {
            free(existing_copy);
        }
        return 0;
    } else {
        trie_node_t *node = trie_traverse(trie, path, true, NULL);
        if (!node) return -1;

        const void *existing = ht_find(node->settings, key, klen, &ulen);
        char *existing_copy = NULL;
        if (existing) {
            existing_copy = *(char**)existing;
            if (ht_remove(node->settings, key, klen) == 0) {
                fprintf(stderr, "[di-vrr] trie_set_config: ht_remove failed for key '%s' (persistent)\n", key);
            }
        }

        if (value) {
            char *v_copy = strdup(value);
            ht_insert_result_t ins_settings = ht_insert(node->settings, key, klen, &v_copy, sizeof(char*));
            if (ins_settings == HT_INSERT_FAILED) {
                free(v_copy);
                if (existing_copy) {
                    ht_insert(node->settings, key, klen, &existing_copy, sizeof(char*));
                }
                return -1;
            }
            /* HT_INSERT_UPDATE or HT_INSERT_OK: existing_copy is superseded, free it */
            if (existing_copy) free(existing_copy);
        } else {
            free(existing_copy);
        }
        return 0;
    }
}

char* trie_get_config(trie_t *trie, const char *path, int fd, const char *key) {
    size_t klen = strlen(key);
    size_t vlen;

    /* 1. Check Transient (FD) */
    const void *found_transient = ht_find(trie->transient_registry, &fd, sizeof(int), &vlen);
    if (found_transient) {
        ht_table_t *kv = *(ht_table_t**)found_transient;
        const void *val = ht_find(kv, key, klen, &vlen);
        if (val) return strdup(*(char**)val);
    }

    /* 2. Check Node and Parents — use same no-strdup segment parsing as trie_traverse */
    char seg_buf[8192];
    size_t seg_count = 0;

    const char *p = path;
    while (*p == '/') p++;
    while (*p && seg_count < 64) {  /* 64 segments × 128 bytes = 8192 bytes total */
        const char *seg_start = p;
        while (*p && *p != '/') p++;
        size_t seg_len = (size_t)(p - seg_start);
        /* Use content comparison (memcmp), not length-based heuristics,
         * to avoid silent collision with method-name-length segments. */
        if (seg_len == 1 && memcmp(seg_start, ".", 1) == 0) {}
        else if (seg_len == 2 && memcmp(seg_start, "..", 2) == 0) { if (seg_count > 0) seg_count--; }
        else {
            if (seg_len > 127) seg_len = 127;  /* cap to fit in 128-byte slot */
            memcpy(seg_buf + seg_count * 128, seg_start, seg_len);
            seg_buf[seg_count * 128 + seg_len] = '\0';
            seg_count++;
        }
        while (*p == '/') p++;
    }

    trie_node_t *nodes[65];
    nodes[0] = trie->root;
    size_t depth = 1;
    for (size_t i = 0; i < seg_count && depth < 65; i++) {
        const char *seg = seg_buf + i * 128;
        trie_node_t *next = node_get_child(nodes[depth - 1], seg, false, NULL);
        if (!next) break;
        nodes[depth++] = next;
    }

    /* Search from deepest found node upward */
    for (int i = (int)depth - 1; i >= 0; i--) {
        const void *val = ht_find(nodes[i]->settings, key, klen, &vlen);
        if (val) return strdup(*(char**)val);
    }

    return NULL;
}

/* --- Persistence --- */

static void persist_escape(const char *src, size_t klen, char *dst, size_t dst_len) {
    size_t dst_pos = 0;
    for (size_t i = 0; i < klen && dst_pos + 1 < dst_len; i++) {
        char c = src[i];
        if (c == '\\' || c == '|' || c == '=' || c == '\n' || c == '\r') {
            if (dst_pos + 2 >= dst_len) break;
            dst[dst_pos++] = '\\';
            switch (c) {
                case '\\': dst[dst_pos++] = '\\'; break;
                case '|':  dst[dst_pos++] = 'c'; break;
                case '=':  dst[dst_pos++] = 'e'; break;
                case '\n': dst[dst_pos++] = 'n'; break;
                case '\r': dst[dst_pos++] = 'r'; break;
            }
        } else {
            dst[dst_pos++] = c;
        }
    }
    dst[dst_pos] = '\0';
}

static void persist_unescape(const char *src, size_t slen, char *dst, size_t dst_len) {
    size_t dst_pos = 0;
    for (size_t i = 0; i < slen && dst_pos + 1 < dst_len; i++) {
        char c = src[i];
        if (c == '\\' && i + 1 < slen) {
            i++;
            switch (src[i]) {
                case '\\': dst[dst_pos++] = '\\'; break;
                case 'c':  dst[dst_pos++] = '|'; break;
                case 'e':  dst[dst_pos++] = '='; break;
                case 'n':  dst[dst_pos++] = '\n'; break;
                case 'r':  dst[dst_pos++] = '\r'; break;
                default:   dst[dst_pos++] = src[i]; break;
            }
        } else {
            dst[dst_pos++] = c;
        }
    }
    dst[dst_pos] = '\0';
}

/* Iterative pre-order traversal to avoid recursion stack overflow on SBC.
 * Each stack entry saves the node, the path length at the start of the node's
 * segment, and the full computed path length so we can backtrack correctly
 * after processing the subtree. Escape buffers are heap-allocated once and
 * reused, eliminating per-frame stack usage. */
#define MAX_SAVE_DEPTH 256
typedef struct {
    trie_node_t *node;
    size_t parent_path_len;  /* path length before this node's segment was appended */
    size_t path_len;         /* full path length after appending this node's segment */
} save_stack_entry_t;

static void node_save_recursive_iterative(trie_node_t *root, FILE *f, char *path_buf, size_t buf_cap, int *truncated) {
    save_stack_entry_t *stack = malloc(sizeof(save_stack_entry_t) * MAX_SAVE_DEPTH);
    char *escaped_path = malloc(8192);
    char *escaped_key = malloc(1024);
    char *escaped_val = malloc(8192);
    if (!stack || !escaped_path || !escaped_key || !escaped_val) {
        fprintf(stderr, "[di-vrr] trie_save_persist: out of memory, skipping save\n");
        free(stack); free(escaped_path); free(escaped_key); free(escaped_val);
        return;
    }
    size_t stack_top = 0;
    /* Root: path="/" (len=1), no parent segment to backtrack through */
    stack[stack_top++] = (save_stack_entry_t){root, 1, 1};

    while (stack_top > 0) {
        save_stack_entry_t entry = stack[--stack_top];
        path_buf[entry.parent_path_len] = '\0';  /* backtrack to parent path */

        /* Save this node's settings */
        ht_iter_t it = ht_iter_begin(entry.node->settings);
        const void *key, *val;
        size_t klen, vlen;
        while (ht_iter_next(entry.node->settings, &it, &key, &klen, &val, &vlen)) {
            /* parent_path_len is at most 8191 (buffer is 8192); guard anyway */
            if (entry.parent_path_len >= 8192) { *truncated = 1; continue; }
            persist_escape(path_buf, entry.parent_path_len, escaped_path, 8192);
            persist_escape((const char*)key, klen, escaped_key, 1024);
            persist_escape(*(char**)val, strlen(*(char**)val), escaped_val, 8192);
            fprintf(f, "%s|%s=%s\n", escaped_path, escaped_key, escaped_val);
        }

        /* Push all children onto stack in reverse order so they process in order.
         * For each child: compute its full path and store both parent_path_len
         * (where child's segment starts) and the computed path_len. */
        size_t child_count = ht_size(entry.node->children);
        if (child_count == 0) continue;

        trie_node_t **children = malloc(sizeof(trie_node_t*) * child_count);
        const void *c_key, *c_val;
        size_t c_klen, c_vlen;
        size_t idx = 0;
        ht_iter_t cit = ht_iter_begin(entry.node->children);
        while (ht_iter_next(entry.node->children, &cit, &c_key, &c_klen, &c_val, &c_vlen)) {
            children[idx++] = *(trie_node_t**)c_val;
        }

        for (size_t i = child_count; i > 0; i--) {
            trie_node_t *child = children[i - 1];
            size_t needed;
            if (entry.parent_path_len == 1) {
                /* At root "/" — child path is "/<segment>" (starts at pos 1) */
                needed = snprintf(path_buf + 1, buf_cap - 1, "%s", child->segment);
            } else {
                /* Not at root — child path is "<parent>/<segment>" (append to parent) */
                needed = snprintf(path_buf + entry.parent_path_len, buf_cap - entry.parent_path_len, "/%s", child->segment);
            }
            if (needed >= buf_cap - entry.parent_path_len) {
                fprintf(stderr, "[di-vrr] trie_save_persist: path overflow, skipping subtree\n");
                *truncated = 1;
                continue;
            }
            /* Store the path length before the child segment for backtracking (parent_path_len)
             * and after the child segment for the child's entry (path_len). */
            size_t child_parent_len = entry.parent_path_len;
            size_t child_full_len = entry.parent_path_len + needed;
            stack[stack_top++] = (save_stack_entry_t){child, child_parent_len, child_full_len};
        }
        free(children);
    }

    free(escaped_path);
    free(escaped_key);
    free(escaped_val);
    free(stack);
}

int trie_save_persist(trie_t *trie, const char *filepath) {
    /* Write to a temp file first, then rename atomically.
     * This prevents a crash during save from corrupting the official persist file. */
    char tmp_path[4096];
    snprintf(tmp_path, sizeof(tmp_path), "%s.tmp", filepath);

    FILE *f = fopen(tmp_path, "w");
    if (!f) return -1;

    char path_buf[8192] = "/";
    int truncated = 0;
    node_save_recursive_iterative(trie->root, f, path_buf, sizeof(path_buf), &truncated);

    if (fclose(f) != 0) {
        int saved_errno = errno;
        if (unlink(tmp_path) < 0) {
            fprintf(stderr, "[di-vrr] trie_save_persist: fclose failed: %s, unlink(%s) also failed: %s\n",
                    strerror(saved_errno), tmp_path, strerror(errno));
        } else {
            fprintf(stderr, "[di-vrr] trie_save_persist: fclose failed: %s, temp unlinked\n",
                    strerror(saved_errno));
        }
        return -1;
    }
    if (truncated) {
        if (unlink(tmp_path) < 0) {
            fprintf(stderr, "[di-vrr] trie_save_persist: path overflow, unlink(%s) failed: %s\n",
                    tmp_path, strerror(errno));
        }
        fprintf(stderr, "[di-vrr] trie_save_persist: path overflow, skipping save\n");
        return -1;
    }

    if (rename(tmp_path, filepath) < 0) {
        int saved_errno = errno;
        if (unlink(tmp_path) < 0) {
            fprintf(stderr, "[di-vrr] trie_save_persist: rename(%s, %s) failed: %s, unlink(%s) also failed: %s\n",
                    tmp_path, filepath, strerror(saved_errno), tmp_path, strerror(errno));
        } else {
            fprintf(stderr, "[di-vrr] trie_save_persist: rename failed: %s, temp unlinked\n",
                    strerror(saved_errno));
        }
        return -1;
    }
    return 0;
}

int trie_load_persist(trie_t *trie, const char *filepath) {
    FILE *f = fopen(filepath, "r");
    if (!f) {
        fprintf(stderr, "[di-vrr] trie_load_persist: fopen(%s) failed: %s\n", filepath, strerror(errno));
        return -1;
    }

    char line[8192];
    while (fgets(line, sizeof(line), f)) {
        line[strcspn(line, "\n")] = 0;
        char *pipe = strchr(line, '|');
        if (!pipe) continue;
        *pipe = '\0';
        char *path = line;
        char *kv = pipe + 1;
        char *eq = strchr(kv, '=');
        if (!eq) continue;
        *eq = '\0';
        char *key = kv;
        char *val = eq + 1;

        char unescaped_path[4096], unescaped_key[256], unescaped_val[8192];
        persist_unescape(path, strlen(path), unescaped_path, sizeof(unescaped_path));
        persist_unescape(key, strlen(key), unescaped_key, sizeof(unescaped_key));
        persist_unescape(val, strlen(val), unescaped_val, sizeof(unescaped_val));

        if (trie_set_config(trie, unescaped_path, -1, unescaped_key, unescaped_val, false) < 0) {
            fprintf(stderr, "[di-vrr] trie_load_persist: failed to set path=%s key=%s\n",
                    unescaped_path, unescaped_key);
        }
    }

    int fclose_err = fclose(f);
    if (fclose_err != 0) {
        fprintf(stderr, "[di-vrr] trie_load_persist: error during read of %s: %s\n", filepath, strerror(errno));
        return -1;
    }
    return 0;
}

/* --- Registry Helpers --- */

/* Returns 0 on success, -1 on allocation failure.
 * Caller must handle failure to avoid orphaned lock state.
 */
static int register_node_to_fd(ht_table_t *registry, trie_node_t *node, int fd) {
    size_t vlen;
    const void *found = ht_find(registry, &fd, sizeof(int), &vlen);
    node_list_t *list;

    if (found) {
        list = *(node_list_t**)found;
    } else {
        list = calloc(1, sizeof(node_list_t));
        if (!list) return -1;
        list->cap = 4;
        list->nodes = malloc(sizeof(trie_node_t*) * list->cap);
        if (!list->nodes) { free(list); return -1; }
        if (ht_upsert(registry, &fd, sizeof(int), &list, sizeof(node_list_t*)) == HT_INSERT_FAILED) {
            free(list->nodes);
            free(list);
            ht_remove(registry, &fd, sizeof(int));
            return -1;
        }
    }

    if (list->count == list->cap) {
        size_t new_cap = list->cap * 2;
        void *tmp = realloc(list->nodes, sizeof(trie_node_t*) * new_cap);
        if (!tmp) {
            /* Do not halve cap — that corrupts the size tracking.
             * Caller sees -1 and unwinds: waiters_count was incremented but
             * fd was NOT added to the array, so state is consistent. */
            fprintf(stderr, "[di-vrr] register_node_to_fd: realloc failed for fd %d\n", fd);
            return -1;
        }
        list->cap = new_cap;
        list->nodes = tmp;
    }
    list->nodes[list->count++] = node;
    return 0;
}

static void unregister_node_from_fd(ht_table_t *registry, trie_node_t *node, int fd) {
    size_t vlen;
    const void *found = ht_find(registry, &fd, sizeof(int), &vlen);
    if (!found) return;
    
    node_list_t *list = *(node_list_t**)found;
    for (size_t i = 0; i < list->count; i++) {
        if (list->nodes[i] == node) {
            list->nodes[i] = list->nodes[list->count - 1];
            list->count--;
            break;
        }
    }
}

/* --- Locking Operations --- */

#define MAX_WAITERS_PER_NODE 4096

int trie_acquire_lock(trie_t *trie, const char *path, int fd, bool wait) {
    if (!path || *path == '\0') return -1;
    bool ancestor_locked = false;
    trie_node_t *current = trie_traverse(trie, path, true, &ancestor_locked);
    if (ancestor_locked || !current) return -1;
    if (current->owner_fd == fd) return 0;  // already owned by this FD — no-op, not a deadlock
    if (current->owner_fd != -1 || current->intent_count > 0) {
        if (!wait) return -1;
        /* Guard against duplicate wait-list entries (e.g. client retries acquire) */
        for (size_t i = 0; i < current->waiters_count; i++)
            if (current->waiters[i] == fd) return 1;  // already waiting, no-op
        if (current->waiters_count >= MAX_WAITERS_PER_NODE) {
            fprintf(stderr, "[di-vrr] trie_acquire_lock: wait queue overflow on path, rejecting fd %d\n", fd);
            return -1;
        }
        /* Doubling growth — initial 32, then double per expansion. Graceful
         * OOM: return -1 and let caller unwind. waiters_count was already
         * incremented but fd was NOT added, so state is consistent. */
        size_t new_cap = current->waiters_count == 0 ? 32 : current->waiters_count * 2;
        if (current->waiters_count >= new_cap) new_cap = current->waiters_count + 1;
        void *tmp = realloc(current->waiters, sizeof(int) * new_cap);
        if (!tmp) {
            fprintf(stderr, "[di-vrr] trie_acquire_lock: realloc failed for waiters of fd %d\n", fd);
            return -1;
        }
        current->waiters = tmp;
        current->waiters[current->waiters_count++] = fd;
        trie->total_waiters++;
        if (register_node_to_fd(trie->waiting_registry, current, fd) < 0) {
            current->waiters_count--;
            return -1;
        }
        return 1;
    }
    current->owner_fd = fd;
    trie->total_locks++;
    if (register_node_to_fd(trie->fd_registry, current, fd) < 0) {
        current->owner_fd = -1;
        return -1;
    }
    trie_node_t *p = current->parent;
    while (p) { p->intent_count++; p = p->parent; }
    return 0;
}

/* Returns the granted FD, or -1 if no waiter / locked / intent blocked.
 * Always performs the grant — caller must handle cap in the cleanup path. */
static int node_grant_to_next_waiter(trie_t *trie, trie_node_t *node) {
    if (node->waiters_count == 0) return -1;
    if (node->intent_count > 0 || node->owner_fd != -1) return -1;
    int next_fd = node->waiters[0];
    memmove(node->waiters, node->waiters + 1, sizeof(int) * (node->waiters_count - 1));
    node->waiters_count--;
    trie->total_waiters--;
    if (node->waiters_count == 0) {
        free(node->waiters);
        node->waiters = NULL;
    }
    unregister_node_from_fd(trie->waiting_registry, node, next_fd);
    node->owner_fd = next_fd;
    trie->total_locks++;
    if (register_node_to_fd(trie->fd_registry, node, next_fd) < 0) {
        node->owner_fd = -1;
        trie->total_locks--;
        return -1;
    }
    trie_node_t *p = node->parent;
    while (p) { p->intent_count++; p = p->parent; }
    return next_fd;
}

int trie_release_lock(trie_t *trie, const char *path, int fd) {
    trie_node_t *current = trie_traverse(trie, path, false, NULL);
    if (!current || current->owner_fd != fd) return -1;
    current->owner_fd = -1;
    trie->total_locks--;
    unregister_node_from_fd(trie->fd_registry, current, fd);
    trie_node_t *p = current->parent;
    while (p) { p->intent_count--; p = p->parent; }
    int next_fd = node_grant_to_next_waiter(trie, current);
    if (next_fd == -1) {
        p = current->parent;
        while (p) {
            next_fd = node_grant_to_next_waiter(trie, p);
            if (next_fd != -1) break;
            p = p->parent;
        }
    }
    return next_fd;
}

size_t trie_get_owned_count(trie_t *trie, int fd) {
    size_t vlen;
    const void *found = ht_find(trie->fd_registry, &fd, sizeof(int), &vlen);
    if (!found) return 0;
    return (*(node_list_t**)found)->count;
}

void trie_get_stats(trie_t *trie, size_t *out_nodes, size_t *out_waiters, size_t *out_locks) {
    if (!trie) { *out_nodes = 0; *out_waiters = 0; *out_locks = 0; return; }
    *out_nodes = trie->total_nodes;
    *out_locks = trie->total_locks;
    *out_waiters = trie->total_waiters;
}

// Recursion depth is bounded by the 256-segment path limit in trie_traverse.
// If that limit is ever removed, this recursion could overflow the stack.
/* Iterative version — builds path by walking up the parent chain, then reverses
 * it into the output buffer. Uses a fixed-size temp array for path segments instead
 * of recursion to avoid stack overflow on deep tries. Max depth 256. */
int node_get_path(trie_node_t *node, char *buf, size_t len) {
    if (!node || !node->parent) {
        if (len > 1) { buf[0] = '/'; buf[1] = '\0'; }
        return len > 1 ? 1 : 0;
    }

    /* Collect segments in reverse order using a fixed-size temp array.
     * Max 256 segments, each up to 127 chars + null = ~32KB worst case,
     * but this is the path, not a per-node scratch buffer. */
    const char *segments[256];
    int depth = 0;
    trie_node_t *n = node;
    while (n->parent && depth < 256) {
        segments[depth++] = n->segment;
        n = n->parent;
    }
    if (depth == 0) { buf[0] = '\0'; return 0; }

    /* Build forward: root "/" then each segment preceded by "/" */
    size_t pos = 0;
    int written = snprintf(buf + pos, len - pos, "/");
    if (written < 0 || (size_t)written >= len - pos) return -1;
    pos += (size_t)written;

    for (int i = depth - 1; i >= 0; i--) {
        written = snprintf(buf + pos, len - pos, "%s", segments[i]);
        if (written < 0 || pos + (size_t)written >= len) return -1;
        pos += (size_t)written;
        if (i > 0) {
            written = snprintf(buf + pos, len - pos, "/");
            if (written < 0 || pos + (size_t)written >= len) return -1;
            pos += (size_t)written;
        }
    }
    return (int)pos;
}

size_t trie_cleanup_fd(trie_t *trie, int fd, int *wakeup, char **paths, size_t wakeup_cap,
                       void (*on_granted)(int, const char*, void*), void *ctx) {
    size_t wakeup_count = 0;
    size_t vlen;

    // 1. FIRST: Remove FD from all waiting registries before releasing any locks.
    //    This prevents the FD from being granted parent locks during its own cleanup
    //    (which would cause a permanent lock leak after the FD is closed).
    const void *found_waiting = ht_find(trie->waiting_registry, &fd, sizeof(int), &vlen);
    if (found_waiting) {
        node_list_t *list = *(node_list_t**)found_waiting;
        for (size_t i = 0; i < list->count; i++) {
            trie_node_t *node = list->nodes[i];
            for (size_t j = 0; j < node->waiters_count; j++) {
                if (node->waiters[j] == fd) {
                    memmove(node->waiters + j, node->waiters + j + 1,
                            sizeof(int) * (node->waiters_count - j - 1));
                    node->waiters_count--;
                    trie->total_waiters--;
                    if (node->waiters_count == 0) {
                        free(node->waiters);
                        node->waiters = NULL;
                    }
                    break;
                }
            }
        }
        ht_remove(trie->waiting_registry, &fd, sizeof(int));
        free(list->nodes);
        free(list);
    }

    // 2. SECOND: Release all owned locks and grant to waiters.
    const void *found_owned = ht_find(trie->fd_registry, &fd, sizeof(int), &vlen);
    if (found_owned) {
        node_list_t *list = *(node_list_t**)found_owned;
        while (list->count > 0) {
            trie_node_t *node = list->nodes[0];
            node->owner_fd = -1;
            trie->total_locks--;
            list->nodes[0] = list->nodes[list->count - 1];
            list->count--;
            char path_buf[4096];
            int path_len = node_get_path(node, path_buf, sizeof(path_buf));
            trie_node_t *p = node->parent;
            while (p) {
                p->intent_count--;
                int w = node_grant_to_next_waiter(trie, p);
                if (w != -1) {
                    if (wakeup_count < wakeup_cap) {
                        wakeup[wakeup_count] = w;
                        paths[wakeup_count] = malloc(4096);
                        if (!paths[wakeup_count]) {
                            /* allocation failed — skip this entry but still invoke callback */
                        } else if (node_get_path(p, paths[wakeup_count], 4096) < 0) {
                            snprintf(paths[wakeup_count], 4096, "<truncated>");
                        }
                        wakeup_count++;
                    }
                    /* Always invoke callback so no grant notification is ever silently dropped,
                     * even when the wakeup array is full. */
                    if (on_granted) {
                        char parent_path[4096];
                        if (node_get_path(p, parent_path, sizeof(parent_path)) >= 0) {
                            on_granted(w, parent_path, ctx);
                        }
                    }
                }
                p = p->parent;
            }
            int w = node_grant_to_next_waiter(trie, node);
            if (w != -1) {
                if (wakeup_count < wakeup_cap) {
                    wakeup[wakeup_count] = w;
                    paths[wakeup_count] = malloc(4096);
                    if (!paths[wakeup_count]) {
                        /* allocation failed — skip entry but still invoke callback */
                    } else if (path_len > 0) {
                        memcpy(paths[wakeup_count], path_buf, path_len + 1);
                    } else {
                        snprintf(paths[wakeup_count], 4096, "<truncated>");
                    }
                    wakeup_count++;
                }
                /* Always invoke callback so no grant notification is ever silently dropped,
                 * even when the wakeup array is full. */
                if (on_granted && path_len > 0) {
                    on_granted(w, path_buf, ctx);
                } else if (on_granted) {
                    on_granted(w, "<unknown>", ctx);
                }
            }
        }
        ht_remove(trie->fd_registry, &fd, sizeof(int));
        free(list->nodes);
        free(list);
    }

    /* Cleanup transient settings */
    const void *found_transient = ht_find(trie->transient_registry, &fd, sizeof(int), &vlen);
    if (found_transient) {
        free_kv_table(*(ht_table_t**)found_transient);
        ht_remove(trie->transient_registry, &fd, sizeof(int));
    }

    node_prune_upward(trie, trie->root);
    return wakeup_count;
}
