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
    int fd = *(const int*)key;
    uint64_t h = (uint64_t)fd;
    h = (h ^ (h >> 30)) * 0xbf58476d1ce4e5b9ULL;
    h = (h ^ (h >> 27)) * 0x94d049bb133111ebULL;
    h = h ^ (h >> 31);
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
        .initial_capacity = 8,
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
        trie_node_t *parent = node->parent;
        size_t seg_len = strlen(node->segment);
        ht_remove(parent->children, node->segment, seg_len);
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
    node_list_t **lists = malloc(sizeof(node_list_t*) * ht_size(registry));
    if (!lists) return;
    size_t count = 0;

    ht_iter_t it = ht_iter_begin(registry);
    const void *key, *val;
    size_t klen, vlen;
    while (ht_iter_next(registry, &it, &key, &klen, &val, &vlen)) {
        lists[count++] = *(node_list_t**)val;
    }
    ht_destroy(registry);

    for (size_t i = 0; i < count; i++) {
        free(lists[i]->nodes);
        free(lists[i]);
    }
    free(lists);
}

static void free_kv_table(ht_table_t *kv) {
    char **vals = malloc(sizeof(char*) * ht_size(kv));
    if (!vals) return;
    size_t count = 0;

    ht_iter_t it = ht_iter_begin(kv);
    const void *key, *val;
    size_t klen, vlen;
    while (ht_iter_next(kv, &it, &key, &klen, &val, &vlen)) {
        vals[count++] = *(char**)val;
    }
    ht_destroy(kv);

    for (size_t i = 0; i < count; i++) free(vals[i]);
    free(vals);
}

void trie_destroy(trie_t *trie) {
    if (!trie) return;
    node_destroy(trie->root);
    free_registry_contents(trie->fd_registry);
    free_registry_contents(trie->waiting_registry);
    
    /* Clean up transient settings */
    ht_table_t **tables = malloc(sizeof(ht_table_t*) * ht_size(trie->transient_registry));
    if (!tables) return;
    size_t count = 0;

    ht_iter_t it = ht_iter_begin(trie->transient_registry);
    const void *key, *val;
    size_t klen, vlen;
    while (ht_iter_next(trie->transient_registry, &it, &key, &klen, &val, &vlen)) {
        tables[count++] = *(ht_table_t**)val;
    }
    ht_destroy(trie->transient_registry);

    for (size_t i = 0; i < count; i++) free_kv_table(tables[i]);
    free(tables);
    
    free(trie);
}

/* --- Trie Helper Operations --- */

static trie_node_t* node_get_child(trie_node_t *node, const char *segment, bool create, trie_t *trie) {
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
    const char *segments[256];
    size_t n_segments = 0;
    char *path_copy = strdup(path);
    char *saveptr;
    char *segment = strtok_r(path_copy, "/", &saveptr);
    while (segment) {
        if (strcmp(segment, ".") == 0) {}
        else if (strcmp(segment, "..") == 0) { if (n_segments > 0) n_segments--; }
        else { if (n_segments < 256) segments[n_segments++] = segment; }
        segment = strtok_r(NULL, "/", &saveptr);
    }
    trie_node_t *current = trie->root;
    for (size_t i = 0; i < n_segments; i++) {
        if (ancestor_locked && current->owner_fd != -1) {
            *ancestor_locked = true;
            free(path_copy);
            return NULL;
        }
        current = node_get_child(current, segments[i], create, trie);
        if (!current) { free(path_copy); return NULL; }
    }
    free(path_copy);
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
            ht_config_t cfg = {.initial_capacity = 8, .max_load_factor = 0.75, .min_load_factor = 0.20, .tomb_threshold = 0.20, .zombie_window = 8};
            kv = ht_create(&cfg, string_hash, string_eq, NULL);
            if (!kv) return -1;
            if (!ht_insert(trie->transient_registry, &fd, sizeof(int), &kv, sizeof(ht_table_t*))) {
                ht_destroy(kv);
                return -1;
            }
        }
        
        const void *existing = ht_find(kv, key, klen, &ulen);
        if (existing) {
            free(*(char**)existing);
            ht_remove(kv, key, klen);
        }

        if (value) {
            char *v_copy = strdup(value);
            if (!ht_insert(kv, key, klen, &v_copy, sizeof(char*))) {
                free(v_copy);
                return -1;
            }
        }
        return 0;
    } else {
        trie_node_t *node = trie_traverse(trie, path, true, NULL);
        if (!node) return -1;

        const void *existing = ht_find(node->settings, key, klen, &ulen);
        if (existing) {
            free(*(char**)existing);
            ht_remove(node->settings, key, klen);
        }

        if (value) {
            char *v_copy = strdup(value);
            if (!ht_insert(node->settings, key, klen, &v_copy, sizeof(char*))) {
                free(v_copy);
                return -1;
            }
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

    /* 2. Check Node and Parents */
    const char *segments[256];
    size_t n_segments = 0;
    char *path_copy = strdup(path);
    char *saveptr;
    char *segment = strtok_r(path_copy, "/", &saveptr);
    while (segment) {
        if (strcmp(segment, ".") == 0) {}
        else if (strcmp(segment, "..") == 0) { if (n_segments > 0) n_segments--; }
        else { if (n_segments < 256) segments[n_segments++] = segment; }
        segment = strtok_r(NULL, "/", &saveptr);
    }

    trie_node_t *nodes[257];
    nodes[0] = trie->root;
    size_t depth = 1;
    for (size_t i = 0; i < n_segments; i++) {
        trie_node_t *next = node_get_child(nodes[depth - 1], segments[i], false, NULL);
        if (!next) break;
        nodes[depth++] = next;
    }
    free(path_copy);

    /* Search from deepest found node upwards */
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
        if (c == '\\' || c == '|' || c == '=' || c == '\n') {
            if (dst_pos + 2 >= dst_len) break;
            dst[dst_pos++] = '\\';
            switch (c) {
                case '\\': dst[dst_pos++] = '\\'; break;
                case '|':  dst[dst_pos++] = 'c'; break;
                case '=':  dst[dst_pos++] = 'e'; break;
                case '\n': dst[dst_pos++] = 'n'; break;
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
                default:   dst[dst_pos++] = src[i]; break;
            }
        } else {
            dst[dst_pos++] = c;
        }
    }
    dst[dst_pos] = '\0';
}

static void node_save_recursive(trie_node_t *node, FILE *f, char *path_buf, size_t buf_cap, int *truncated) {
    size_t path_len = strlen(path_buf);

    /* Save settings for this node */
    ht_iter_t it = ht_iter_begin(node->settings);
    const void *key, *val;
    size_t klen, vlen;
    while (ht_iter_next(node->settings, &it, &key, &klen, &val, &vlen)) {
        char escaped_path[8192], escaped_key[1024], escaped_val[8192];
        persist_escape(path_buf, strlen(path_buf), escaped_path, sizeof(escaped_path));
        persist_escape((const char*)key, klen, escaped_key, sizeof(escaped_key));
        persist_escape(*(char**)val, strlen(*(char**)val), escaped_val, sizeof(escaped_val));
        fprintf(f, "%s|%s=%s\n", escaped_path, escaped_key, escaped_val);
    }

    /* Recurse into children */
    ht_iter_t cit = ht_iter_begin(node->children);
    while (ht_iter_next(node->children, &cit, &key, &klen, &val, &vlen)) {
        trie_node_t *child = *(trie_node_t**)val;

        if (strcmp(path_buf, "/") == 0) {
            if ((size_t)snprintf(path_buf + 1, buf_cap - 1, "%s", child->segment) >= buf_cap - 1) {
                fprintf(stderr, "[di-vrr] trie_save_persist: path overflow at /%s, skipping subtree\n", child->segment);
                *truncated = 1;
                continue;
            }
        } else {
            if ((size_t)snprintf(path_buf + path_len, buf_cap - path_len, "/%s", child->segment) >= buf_cap - path_len) {
                fprintf(stderr, "[di-vrr] trie_save_persist: path overflow at %s/%s, skipping subtree\n", path_buf, child->segment);
                *truncated = 1;
                continue;
            }
        }

        node_save_recursive(child, f, path_buf, buf_cap, truncated);
        path_buf[path_len] = '\0'; /* Backtrack */
    }
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
    node_save_recursive(trie->root, f, path_buf, sizeof(path_buf), &truncated);

    fclose(f);
    if (truncated) {
        unlink(tmp_path);
        fprintf(stderr, "[di-vrr] trie_save_persist: path overflow, skipping save\n");
        return -1;
    }

    if (rename(tmp_path, filepath) < 0) {
        unlink(tmp_path);
        fprintf(stderr, "[di-vrr] trie_save_persist: rename failed: %s\n", strerror(errno));
        return -1;
    }
    return 0;
}

int trie_load_persist(trie_t *trie, const char *filepath) {
    FILE *f = fopen(filepath, "r");
    if (!f) return -1;

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

        trie_set_config(trie, unescaped_path, -1, unescaped_key, unescaped_val, false);
    }

    fclose(f);
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
            return -1;
        }
    }

    if (list->count == list->cap) {
        list->cap *= 2;
        void *tmp = realloc(list->nodes, sizeof(trie_node_t*) * list->cap);
        if (!tmp) { list->cap /= 2; return -1; }
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
        void *tmp = realloc(current->waiters, sizeof(int) * (current->waiters_count + 1));
        if (!tmp) return -1;
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
int node_get_path(trie_node_t *node, char *buf, size_t len) {
    if (!node || !node->parent) {
        if (len > 1) { buf[0] = '/'; buf[1] = '\0'; }
        return len > 1 ? 1 : 0;
    }

    char temp[4096];
    int n = node_get_path(node->parent, temp, sizeof(temp));
    if (n < 0) return -1;

    size_t available = len;
    int written;
    if (n == 1 && temp[0] == '/') {
        written = snprintf(buf, available, "/%s", node->segment);
    } else {
        written = snprintf(buf, available, "%s/%s", temp, node->segment);
    }
    if (written < 0) return -1;
    if ((size_t)written >= len) return -1;
    return written;
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
        free(list->nodes);
        free(list);
        ht_remove(trie->waiting_registry, &fd, sizeof(int));
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
                        if (node_get_path(p, paths[wakeup_count], 4096) < 0) {
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
                    if (path_len > 0) {
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
        free(list->nodes);
        free(list);
        ht_remove(trie->fd_registry, &fd, sizeof(int));
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
