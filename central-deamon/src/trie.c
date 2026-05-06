#include "trie.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

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
    
    if (segment) node->segment = strdup(segment);
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
    free(node->segment);
    free(node->waiters);
    free(node);
}

trie_t* trie_create(void) {
    trie_t *trie = malloc(sizeof(trie_t));
    trie->root = node_create(NULL, NULL);
    
    ht_config_t cfg = {
        .initial_capacity = 64,
        .max_load_factor = 0.75,
        .min_load_factor = 0.20,
        .tomb_threshold = 0.20,
        .zombie_window = 16
    };
    trie->fd_registry = ht_create(&cfg, fd_hash, fd_eq, NULL);
    trie->waiting_registry = ht_create(&cfg, fd_hash, fd_eq, NULL);
    
    return trie;
}

static void free_registry_contents(ht_table_t *registry) {
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

void trie_destroy(trie_t *trie) {
    if (!trie) return;
    node_destroy(trie->root);
    free_registry_contents(trie->fd_registry);
    free_registry_contents(trie->waiting_registry);
    free(trie);
}

/* Registry Helpers */

static void register_node_to_fd(ht_table_t *registry, trie_node_t *node, int fd) {
    size_t vlen;
    const void *found = ht_find(registry, &fd, sizeof(int), &vlen);
    node_list_t *list;
    
    if (found) {
        list = *(node_list_t**)found;
    } else {
        list = calloc(1, sizeof(node_list_t));
        list->cap = 4;
        list->nodes = malloc(sizeof(trie_node_t*) * list->cap);
        ht_upsert(registry, &fd, sizeof(int), &list, sizeof(node_list_t*));
    }
    
    if (list->count == list->cap) {
        list->cap *= 2;
        list->nodes = realloc(list->nodes, sizeof(trie_node_t*) * list->cap);
    }
    list->nodes[list->count++] = node;
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

/* Trie Operations */

static trie_node_t* node_get_child(trie_node_t *node, const char *segment, bool create) {
    size_t segment_len = strlen(segment);
    size_t vlen;
    const void *found = ht_find(node->children, segment, segment_len, &vlen);
    
    if (found) return *(trie_node_t**)found;
    if (!create) return NULL;
    
    trie_node_t *new_node = node_create(segment, node);
    if (!new_node) return NULL;
    
    ht_insert(node->children, segment, segment_len, &new_node, sizeof(trie_node_t*));
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
        current = node_get_child(current, segments[i], create);
        if (!current) { free(path_copy); return NULL; }
    }
    free(path_copy);
    return current;
}

int trie_acquire_lock(trie_t *trie, const char *path, int fd, bool wait) {
    if (!path || *path == '\0') return -1;
    bool ancestor_locked = false;
    trie_node_t *current = trie_traverse(trie, path, true, &ancestor_locked);
    if (ancestor_locked || !current) return -1;
    if (current->owner_fd != -1 || current->intent_count > 0) {
        if (!wait) return -1; /* Lock is held, and client doesn't want to wait */
        current->waiters = realloc(current->waiters, sizeof(int) * (current->waiters_count + 1));
        current->waiters[current->waiters_count++] = fd;
        register_node_to_fd(trie->waiting_registry, current, fd);
        return 1;
    }
    current->owner_fd = fd;
    register_node_to_fd(trie->fd_registry, current, fd);
    trie_node_t *p = current->parent;
    while (p) { p->intent_count++; p = p->parent; }
    return 0;
}

static int node_grant_to_next_waiter(trie_t *trie, trie_node_t *node) {
    if (node->waiters_count == 0 || node->intent_count > 0 || node->owner_fd != -1) return -1;
    int next_fd = node->waiters[0];
    memmove(node->waiters, node->waiters + 1, sizeof(int) * (node->waiters_count - 1));
    node->waiters_count--;
    unregister_node_from_fd(trie->waiting_registry, node, next_fd);
    node->owner_fd = next_fd;
    register_node_to_fd(trie->fd_registry, node, next_fd);
    trie_node_t *p = node->parent;
    while (p) { p->intent_count++; p = p->parent; }
    return next_fd;
}

int trie_release_lock(trie_t *trie, const char *path, int fd) {
    trie_node_t *current = trie_traverse(trie, path, false, NULL);
    if (!current || current->owner_fd != fd) return -1;
    current->owner_fd = -1;
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

void node_get_path(trie_node_t *node, char *buf, size_t len) {
    if (!node || !node->parent) {
        if (len > 1) { buf[0] = '/'; buf[1] = '\0'; }
        return;
    }
    
    char temp[4096];
    node_get_path(node->parent, temp, sizeof(temp));
    
    if (strcmp(temp, "/") == 0) {
        snprintf(buf, len, "/%s", node->segment);
    } else {
        snprintf(buf, len, "%s/%s", temp, node->segment);
    }
}

size_t trie_cleanup_fd(trie_t *trie, int fd, int *wakeup, char **paths, size_t wakeup_cap) {
    size_t wakeup_count = 0;
    size_t vlen;
    const void *found_owned = ht_find(trie->fd_registry, &fd, sizeof(int), &vlen);
    if (found_owned) {
        node_list_t *list = *(node_list_t**)found_owned;
        while (list->count > 0) {
            trie_node_t *node = list->nodes[0];
            node->owner_fd = -1;
            list->nodes[0] = list->nodes[list->count - 1];
            list->count--;
            trie_node_t *p = node->parent;
            while (p) {
                p->intent_count--;
                int w = node_grant_to_next_waiter(trie, p);
                if (w != -1 && wakeup_count < wakeup_cap) {
                    wakeup[wakeup_count] = w;
                    paths[wakeup_count] = malloc(4096);
                    node_get_path(p, paths[wakeup_count], 4096);
                    wakeup_count++;
                }
                p = p->parent;
            }
            int w = node_grant_to_next_waiter(trie, node);
            if (w != -1 && wakeup_count < wakeup_cap) {
                wakeup[wakeup_count] = w;
                paths[wakeup_count] = malloc(4096);
                node_get_path(node, paths[wakeup_count], 4096);
                wakeup_count++;
            }
        }
        free(list->nodes);
        free(list);
        ht_remove(trie->fd_registry, &fd, sizeof(int));
    }
    const void *found_waiting = ht_find(trie->waiting_registry, &fd, sizeof(int), &vlen);
    if (found_waiting) {
        node_list_t *list = *(node_list_t**)found_waiting;
        for (size_t i = 0; i < list->count; i++) {
            trie_node_t *node = list->nodes[i];
            for (size_t j = 0; j < node->waiters_count; j++) {
                if (node->waiters[j] == fd) {
                    memmove(node->waiters + j, node->waiters + j + 1, sizeof(int) * (node->waiters_count - j - 1));
                    node->waiters_count--;
                    break;
                }
            }
        }
        free(list->nodes);
        free(list);
        ht_remove(trie->waiting_registry, &fd, sizeof(int));
    }
    return wakeup_count;
}
