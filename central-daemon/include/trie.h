#ifndef DI_VRR_TRIE_H
#define DI_VRR_TRIE_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <draugr/ht.h>

/**
 * trie_node_t - A node in the path-based coordination trie.
 * 
 * This structure facilitates hierarchical locking.
 */
typedef struct trie_node {
    char *segment;              /* The path segment name (e.g. "src") */
    struct trie_node *parent;   /* Parent pointer for upward propagation */
    
    /* Child management - using Draugr High-performance Hash Table */
    ht_table_t *children;       /* Key: segment string, Value: trie_node_t* */

    /* Settings storage (Project/Global layer) */
    ht_table_t *settings;       /* Key: string, Value: string (Value string) */

    /* Lock state */
    int owner_fd;               /* FD of the connection holding an exclusive lock on this path */
    int64_t intent_count;       /* Number of exclusive locks in the subtree below this node */
    
    /* Wait queue - list of FDs waiting for this node to become available */
    int *waiters;
    size_t waiters_count;
} trie_node_t;

/**
 * trie_t - The root of the coordination trie.
 */
typedef struct {
    trie_node_t *root;
    ht_table_t *fd_registry;    /* Key: int fd, Value: node_list_t* (Owned nodes) */
    ht_table_t *waiting_registry; /* Key: int fd, Value: node_list_t* (Nodes being waited on) */
    ht_table_t *transient_registry; /* Key: int fd, Value: ht_table_t* (Per-connection KV overrides) */

    /* O(1) cumulative stats — updated at every mutation point */
    size_t total_nodes;      /* total nodes in the trie */
    size_t total_locks;      /* nodes with owner_fd != -1 */
    size_t total_waiters;    /* sum of all waiters_count */
} trie_t;

/* Core Trie Operations */
trie_t* trie_create(void);
void trie_destroy(trie_t *trie);

/* Configuration Management */
int trie_set_config(trie_t *trie, const char *path, int fd, const char *key, const char *value, bool transient);
char* trie_get_config(trie_t *trie, const char *path, int fd, const char *key);

/* Persistence */
int trie_load_persist(trie_t *trie, const char *filepath);
int trie_save_persist(trie_t *trie, const char *filepath);

/**
 * trie_acquire_lock - Attempt to acquire a lock on a path.
 * 
 * @param path The absolute or relative path to lock.
 * @param fd The connection FD requesting the lock.
 * @return 0 if granted, 1 if added to wait queue, -1 if denied (error).
 */
int trie_acquire_lock(trie_t *trie, const char *path, int fd, bool wait);

/**
 * trie_release_lock - Release a lock held by an FD.
 * 
 * @param path The path to release.
 * @param fd The connection FD.
 * @return The next FD in the wait queue for this path, or -1 if no one is waiting.
 */
int trie_release_lock(trie_t *trie, const char *path, int fd);

/**
 * trie_traverse - Resolve a path to its corresponding node.
 */
trie_node_t* trie_traverse(trie_t *trie, const char *path, bool create, bool *ancestor_locked);

/**
 * trie_cleanup_fd - Release all locks held by a specific FD (on disconnect).
 *
 * @param wakeup An array of ints to store FDs that were granted locks.
 * @param paths An array of path strings for each granted FD.
 * @param wakeup_cap The size of the wakeup array.
 * @param on_granted Called for every grant that occurs (including when the
 *                   wakeup array is full and notification is dropped from the array).
 *                   If NULL, no callback is invoked.
 * @return The number of FDs added to the wakeup array.
 */
size_t trie_cleanup_fd(trie_t *trie, int fd, int *wakeup, char **paths,
                      size_t wakeup_cap,
                      void (*on_granted)(int granted_fd, const char *path, void *ctx),
                      void *ctx);

/* Path reconstruction. Returns path length written, or -1 on truncation. */
int node_get_path(trie_node_t *node, char *buf, size_t len);

/**
 * trie_get_owned_count - Helper for testing cleanup.
 */
size_t trie_get_owned_count(trie_t *trie, int fd);

/**
 * trie_get_stats - Fill output counters by walking the entire trie.
 *   *out_nodes   = total trie nodes
 *   *out_waiters = sum of waiters_count across all nodes
 *   *out_locks   = nodes with owner_fd != -1
 */
void trie_get_stats(trie_t *trie, size_t *out_nodes, size_t *out_waiters, size_t *out_locks);

#endif /* DI_VRR_TRIE_H */
