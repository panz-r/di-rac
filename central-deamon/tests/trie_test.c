#include <stdio.h>
#include <stdlib.h>
#include <assert.h>
#include <string.h>
#include "trie.h"

void test_basic_locking() {
    printf("Testing basic locking...\n");
    trie_t *t = trie_create();
    assert(trie_acquire_lock(t, "/a/b", 10, true) == 0);
    assert(trie_acquire_lock(t, "/a/b", 11, true) == 1);
    int next = trie_release_lock(t, "/a/b", 10);
    assert(next == 11);
    trie_destroy(t);
    printf("PASS\n");
}

void test_hierarchical_locking() {
    printf("Testing hierarchical locking...\n");
    trie_t *t = trie_create();
    assert(trie_acquire_lock(t, "/work", 10, true) == 0);
    assert(trie_acquire_lock(t, "/work/project", 11, true) == -1);
    trie_release_lock(t, "/work", 10);
    assert(trie_acquire_lock(t, "/work/project/src", 12, true) == 0);
    assert(trie_acquire_lock(t, "/work", 13, true) == 1);
    trie_release_lock(t, "/work/project/src", 12);
    trie_destroy(t);
    printf("PASS\n");
}

void test_cleanup() {
    printf("Testing cleanup on disconnect...\n");
    trie_t *t = trie_create();
    trie_acquire_lock(t, "/a", 10, true);
    trie_acquire_lock(t, "/b", 10, true);
    assert(trie_get_owned_count(t, 10) == 2);
    assert(trie_acquire_lock(t, "/a", 11, true) == 1);
    int wakeup[16];
    char *w_paths[16];
    size_t count = trie_cleanup_fd(t, 10, wakeup, w_paths, 16);
    for (size_t i = 0; i < count; i++) free(w_paths[i]);
    assert(trie_get_owned_count(t, 10) == 0);
    assert(trie_get_owned_count(t, 11) == 1);
    assert(trie_acquire_lock(t, "/b", 12, true) == 0);
    trie_destroy(t);
    printf("PASS\n");
}

void test_deep_hierarchy() {
    printf("Testing deep hierarchy consistency...\n");
    trie_t *t = trie_create();
    assert(trie_acquire_lock(t, "/a/b/c/d/e", 10, true) == 0);
    assert(trie_acquire_lock(t, "/a/x", 11, true) == 0);
    trie_destroy(t);
    printf("PASS\n");
}

void test_multi_waiter_queue() {
    printf("Testing multi-waiter FIFO queue...\n");
    trie_t *t = trie_create();
    assert(trie_acquire_lock(t, "/lock", 10, true) == 0);
    assert(trie_acquire_lock(t, "/lock", 11, true) == 1);
    assert(trie_acquire_lock(t, "/lock", 12, true) == 1);
    assert(trie_acquire_lock(t, "/lock", 13, true) == 1);
    assert(trie_release_lock(t, "/lock", 10) == 11);
    assert(trie_release_lock(t, "/lock", 11) == 12);
    assert(trie_release_lock(t, "/lock", 12) == 13);
    assert(trie_release_lock(t, "/lock", 13) == -1);
    trie_destroy(t);
    printf("PASS\n");
}

void test_intent_lock_safety() {
    printf("Testing intent lock collision safety...\n");
    trie_t *t = trie_create();
    assert(trie_acquire_lock(t, "/work/project/A", 10, true) == 0);
    assert(trie_acquire_lock(t, "/work", 11, true) == 1);
    assert(trie_acquire_lock(t, "/work/project/B", 12, true) == 0);
    /* Releasing child A wakes up NO ONE because child B still has an intent lock on /work */
    assert(trie_release_lock(t, "/work/project/A", 10) == -1); 
    /* Releasing child B finally wakes up the /work waiter (11) */
    assert(trie_release_lock(t, "/work/project/B", 12) == 11);
    trie_destroy(t);
    printf("PASS\n");
}

void test_edge_cases() {
    printf("Testing edge cases...\n");
    trie_t *t = trie_create();
    assert(trie_acquire_lock(t, "", 10, true) == -1);
    assert(trie_release_lock(t, "/ghost", 10) == -1);
    trie_acquire_lock(t, "/real", 10, true);
    assert(trie_release_lock(t, "/real", 11) == -1);
    assert(trie_acquire_lock(t, "/real", 10, true) == 1);
    trie_destroy(t);
    printf("PASS\n");
}

void test_path_normalization() {
    printf("Testing path normalization (redundant slashes)...\n");
    trie_t *t = trie_create();
    assert(trie_acquire_lock(t, "/a//b///c", 10, true) == 0);
    assert(trie_acquire_lock(t, "/a/b/c", 11, true) == 1); 
    assert(trie_acquire_lock(t, "/x/y/", 12, true) == 0);
    assert(trie_acquire_lock(t, "/x/y", 13, true) == 1);
    trie_destroy(t);
    printf("PASS\n");
}

void test_multiple_blockers() {
    printf("Testing multiple blockers for a single parent...\n");
    trie_t *t = trie_create();
    assert(trie_acquire_lock(t, "/a/1", 10, true) == 0);
    assert(trie_acquire_lock(t, "/a/2", 11, true) == 0);
    assert(trie_acquire_lock(t, "/a", 13, true) == 1);
    assert(trie_release_lock(t, "/a/1", 10) == -1);
    assert(trie_release_lock(t, "/a/2", 11) == 13);
    trie_destroy(t);
    printf("PASS\n");
}

void test_waiter_abandonment() {
    printf("Testing waiter abandonment (disconnect while waiting)...\n");
    trie_t *t = trie_create();
    assert(trie_acquire_lock(t, "/lock", 10, true) == 0);
    assert(trie_acquire_lock(t, "/lock", 11, true) == 1);
    assert(trie_acquire_lock(t, "/lock", 12, true) == 1);
    int wakeup[16];
    char *w_paths[16];
    size_t count = trie_cleanup_fd(t, 11, wakeup, w_paths, 16);
    for (size_t i = 0; i < count; i++) free(w_paths[i]);
    int next = trie_release_lock(t, "/lock", 10);
    assert(next == 12); 
    assert(trie_get_owned_count(t, 12) == 1);
    trie_destroy(t);
    printf("PASS\n");
}

void test_complex_tree() {
    printf("Testing complex tree intersections...\n");
    trie_t *t = trie_create();
    assert(trie_acquire_lock(t, "/v/src/a.c", 10, true) == 0);
    assert(trie_acquire_lock(t, "/v/tests/b.test", 11, true) == 0);
    assert(trie_acquire_lock(t, "/v", 12, true) == 1);
    assert(trie_acquire_lock(t, "/v/src/c.c", 13, true) == 0); 
    int wakeup[16];
    char *w_paths[16];
    size_t count = trie_cleanup_fd(t, 10, wakeup, w_paths, 16);
    for (size_t i = 0; i < count; i++) free(w_paths[i]);
    count = trie_cleanup_fd(t, 11, wakeup, w_paths, 16);
    for (size_t i = 0; i < count; i++) free(w_paths[i]);
    assert(trie_release_lock(t, "/v/src/c.c", 13) == 12);
    trie_destroy(t);
    printf("PASS\n");
}

void test_path_normalization_extreme() {
    printf("Testing extreme path normalization...\n");
    trie_t *t = trie_create();
    
    /* 1. Acquire with complex path */
    int res = trie_acquire_lock(t, "///v/./src/../src///", 10, true);
    assert(res == 0);
    
    /* 2. Release with different but equivalent string.
       Should return -1 because no one is waiting. 
       If it returns -2 or similar it would be an error. */
    res = trie_release_lock(t, "/v/src", 10);
    assert(res == -1); /* Successfully released, no waiters */
    
    /* 3. Verify it was actually released by locking again */
    assert(trie_acquire_lock(t, "/v/src", 11, true) == 0);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_intent_lock_starvation() {
    printf("Testing intent lock exhaustion/starvation...\n");
    trie_t *t = trie_create();
    
    /* Lock 100 deep children of /v */
    for (int i = 0; i < 100; i++) {
        char path[64];
        sprintf(path, "/v/child_%d", i);
        assert(trie_acquire_lock(t, path, 100 + i, true) == 0);
    }
    
    /* Someone tries to lock /v exclusively */
    assert(trie_acquire_lock(t, "/v", 200, true) == 1); // Must wait
    
    /* Release all children one by one */
    for (int i = 0; i < 99; i++) {
        char path[64];
        sprintf(path, "/v/child_%d", i);
        assert(trie_release_lock(t, path, 100 + i) == -1); // No one waiting for child
    }
    
    /* Release last child. This should wake up 200. */
    assert(trie_release_lock(t, "/v/child_99", 199) == 200);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_path_traversal_above_root() {
    printf("Testing path traversal above root (../../..)...\n");
    trie_t *t = trie_create();
    
    int r1 = trie_acquire_lock(t, "/", 10, true);
    printf(" - ACQ /: %d\n", r1);
    assert(r1 == 0);
    
    int r2 = trie_acquire_lock(t, "/..", 11, true);
    printf(" - ACQ /..: %d\n", r2);
    assert(r2 == 1);
    
    int r3 = trie_acquire_lock(t, "/a/../..", 12, true);
    printf(" - ACQ /a/../..: %d\n", r3);
    assert(r3 == 1);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_waiter_reordering() {
    printf("Testing waiter reordering/subset disconnects...\n");
    trie_t *t = trie_create();
    
    trie_acquire_lock(t, "/res", 10, true);
    trie_acquire_lock(t, "/res", 11, true); // W1
    trie_acquire_lock(t, "/res", 12, true); // W2
    trie_acquire_lock(t, "/res", 13, true); // W3
    trie_acquire_lock(t, "/res", 14, true); // W4
    
    /* W1 and W3 disconnect */
    int wakeup[16];
    char *w_paths[16];
    size_t count = trie_cleanup_fd(t, 11, wakeup, w_paths, 16);
    for (size_t i = 0; i < count; i++) free(w_paths[i]);
    count = trie_cleanup_fd(t, 13, wakeup, w_paths, 16);
    for (size_t i = 0; i < count; i++) free(w_paths[i]);
    
    /* Release owner 10. Next should be W2 (12). */
    assert(trie_release_lock(t, "/res", 10) == 12);
    /* Release 12. Next should be W4 (14). */
    assert(trie_release_lock(t, "/res", 12) == 14);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_intent_churn() {
    printf("Testing intent lock consistency under high churn...\n");
    trie_t *t = trie_create();
    
    /* 1. Repeatedly lock/unlock deep children */
    for (int i = 0; i < 1000; i++) {
        trie_acquire_lock(t, "/v/deep/path/to/resource", 10, true);
        trie_release_lock(t, "/v/deep/path/to/resource", 10);
    }
    
    /* 2. Intent count should be exactly 0 at all levels */
    trie_node_t *node = t->root;
    const char *path[] = {"v", "deep", "path", "to"};
    for (int i = 0; i < 4; i++) {
        size_t vlen;
        void *found = ht_find(node->children, path[i], strlen(path[i]), &vlen);
        node = *(trie_node_t**)found;
        assert(node->intent_count == 0);
    }
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_massive_waiter_queue() {
    printf("Testing massive waiter queue (1000 waiters)...\n");
    trie_t *t = trie_create();
    
    trie_acquire_lock(t, "/bottle-neck", 10, true);
    for (int i = 0; i < 1000; i++) {
        assert(trie_acquire_lock(t, "/bottle-neck", 100 + i, true) == 1);
    }
    
    /* Release should grant to first in line */
    assert(trie_release_lock(t, "/bottle-neck", 10) == 100);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_cleanup_recursive_intent() {
    printf("Testing recursive intent cleanup on disconnect...\n");
    trie_t *t = trie_create();
    
    /* 10 owns two deep resources */
    trie_acquire_lock(t, "/a/b/c", 10, true);
    trie_acquire_lock(t, "/a/b/d", 10, true);
    
    /* /a/b should have intent_count = 2 */
    trie_node_t *ab = trie_traverse(t, "/a/b", false, NULL);
    assert(ab->intent_count == 2);
    
    /* 10 disconnects */
    int wakeup[16];
    char *w_paths[16];
    size_t count = trie_cleanup_fd(t, 10, wakeup, w_paths, 16);
    for (size_t i = 0; i < count; i++) free(w_paths[i]);
    
    /* intent_count must be 0 */
    assert(ab->intent_count == 0);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_oversized_path() {
    printf("Testing oversized path (8KB)...\n");
    trie_t *t = trie_create();
    
    char big_path[8192];
    memset(big_path, 'a', sizeof(big_path));
    for (int i = 0; i < 8191; i += 50) {
        big_path[i] = '/';
    }
    big_path[8191] = '\0';
    
    assert(trie_acquire_lock(t, big_path, 10, true) == 0);
    assert(trie_release_lock(t, big_path, 10) == -1);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_deep_intent_hierarchy_complex() {
    printf("Testing deep intent hierarchy (recursive stress)...\n");
    trie_t *t = trie_create();
    
    /* Lock /a/b/c/d/e/f/g/h/i/j exclusively */
    trie_acquire_lock(t, "/a/b/c/d/e/f/g/h/i/j", 10, true);
    
    /* Parent /a should have intent_count = 1 */
    trie_node_t *node = trie_traverse(t, "/a", false, NULL);
    assert(node->intent_count == 1);
    
    /* Lock /a/b/c/d/e/f/x exclusively (different branch) */
    trie_acquire_lock(t, "/a/b/c/d/e/f/x", 11, true);
    
    /* /a/b/c/d/e/f should now have intent_count = 2 */
    node = trie_traverse(t, "/a/b/c/d/e/f", false, NULL);
    assert(node->intent_count == 2);
    
    /* Release both */
    trie_release_lock(t, "/a/b/c/d/e/f/g/h/i/j", 10);
    trie_release_lock(t, "/a/b/c/d/e/f/x", 11);
    
    /* /a should have intent_count = 0 */
    node = trie_traverse(t, "/a", false, NULL);
    assert(node->intent_count == 0);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_path_segment_limit() {
    printf("Testing path segment limit (256+ segments)...\n");
    trie_t *t = trie_create();
    
    char long_path[2048] = {0};
    for (int i = 0; i < 300; i++) {
        strcat(long_path, "/a");
    }
    
    /* The current implementation truncates to 256 segments.
       Let's verify it doesn't crash and behaves predictably. */
    assert(trie_acquire_lock(t, long_path, 10, true) == 0);
    
    /* A path with 256 segments should match it. */
    char match_path[2048] = {0};
    for (int i = 0; i < 256; i++) {
        strcat(match_path, "/a");
    }
    assert(trie_release_lock(t, match_path, 10) == -1);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_cleanup_massive_wakeups() {
    printf("Testing trie_cleanup_fd with massive wakeups (> cap)...\n");
    trie_t *t = trie_create();
    
    /* 10 owns /root */
    trie_acquire_lock(t, "/root", 10, true);
    
    /* 100 people waiting for /root */
    for (int i = 0; i < 100; i++) {
        trie_acquire_lock(t, "/root", 100 + i, true);
    }
    
    /* Cleanup 10 with cap of 16 */
    int wakeup[16];
    char *w_paths[16];
    size_t count = trie_cleanup_fd(t, 10, wakeup, w_paths, 16);
    
    /* Only 16 should be woken up in the array, but lock is granted to 100. */
    assert(count == 1); /* Only the direct waiter of /root is woken up */
    assert(wakeup[0] == 100);
    free(w_paths[0]);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_deep_recursive_parent_wakeup() {
    printf("Testing deep recursive parent wakeup...\n");
    trie_t *t = trie_create();
    
    /* /a/b/c is locked by 10 */
    trie_acquire_lock(t, "/a/b/c", 10, true);
    
    /* /a is waited by 11 */
    assert(trie_acquire_lock(t, "/a", 11, true) == 1);
    
    /* Releasing /a/b/c should wake up 11 because /a is now free of intents */
    assert(trie_release_lock(t, "/a/b/c", 10) == 11);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_cleanup_multi_branch_wakeup() {
    printf("Testing cleanup with multi-branch wakeups...\n");
    trie_t *t = trie_create();
    
    /* 10 owns two unrelated nodes */
    trie_acquire_lock(t, "/a/1", 10, true);
    trie_acquire_lock(t, "/b/1", 10, true);
    
    /* 11 waits for /a */
    assert(trie_acquire_lock(t, "/a", 11, true) == 1);
    /* 12 waits for /b */
    assert(trie_acquire_lock(t, "/b", 12, true) == 1);
    
    int wakeup[16];
    char *w_paths[16];
    size_t count = trie_cleanup_fd(t, 10, wakeup, w_paths, 16);
    
    /* Both should be woken up */
    assert(count == 2);
    /* Order depends on which one was processed first in the registry loop, 
       but both must be present. */
    bool found11 = false, found12 = false;
    for (size_t i = 0; i < count; i++) {
        if (wakeup[i] == 11) {
            found11 = true;
            assert(strcmp(w_paths[i], "/a") == 0);
        }
        if (wakeup[i] == 12) {
            found12 = true;
            assert(strcmp(w_paths[i], "/b") == 0);
        }
        free(w_paths[i]);
    }
    assert(found11 && found12);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_breadth_stress() {
    printf("Testing breadth stress (1000 children)...\n");
    trie_t *t = trie_create();
    for (int i = 0; i < 1000; i++) {
        char path[32];
        sprintf(path, "/root/child_%d", i);
        assert(trie_acquire_lock(t, path, 1000 + i, true) == 0);
    }
    
    /* Releasing all should be fast */
    for (int i = 0; i < 1000; i++) {
        char path[32];
        sprintf(path, "/root/child_%d", i);
        assert(trie_release_lock(t, path, 1000 + i) == -1);
    }
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_denied_on_ancestor_lock() {
    printf("Testing denied on ancestor lock...\n");
    trie_t *t = trie_create();
    
    /* Lock /a exclusively */
    assert(trie_acquire_lock(t, "/a", 10, true) == 0);
    
    /* Try to lock /a/b - should be denied (-1), not waiting (1) */
    assert(trie_acquire_lock(t, "/a/b", 11, true) == -1);
    
    /* Try to lock /a/b/c - should also be denied */
    assert(trie_acquire_lock(t, "/a/b/c", 12, true) == -1);
    
    trie_release_lock(t, "/a", 10);
    
    /* Now /a/b can be locked */
    assert(trie_acquire_lock(t, "/a/b", 13, true) == 0);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_cleanup_mixed_state() {
    printf("Testing cleanup with mixed owned/waiting state...\n");
    trie_t *t = trie_create();
    
    /* 10 owns /owned */
    trie_acquire_lock(t, "/owned", 10, true);
    
    /* 11 owns /other */
    trie_acquire_lock(t, "/other", 11, true);
    
    /* 10 waits for /other */
    assert(trie_acquire_lock(t, "/other", 10, true) == 1);
    
    /* 12 waits for /owned */
    assert(trie_acquire_lock(t, "/owned", 12, true) == 1);
    
    int wakeup[16];
    char *w_paths[16];
    size_t count = trie_cleanup_fd(t, 10, wakeup, w_paths, 16);
    
    /* 10's release of /owned should wake up 12 */
    assert(count == 1);
    assert(wakeup[0] == 12);
    assert(strcmp(w_paths[0], "/owned") == 0);
    free(w_paths[0]);
    
    /* /other should still be owned by 11 */
    assert(trie_get_owned_count(t, 11) == 1);
    
    /* Releasing /other by 11 should wake up NO ONE (10 is gone) */
    assert(trie_release_lock(t, "/other", 11) == -1);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_cascading_intent_decrement() {
    printf("Testing cascading intent decrement on release...\n");
    trie_t *t = trie_create();
    
    /* Lock /a/b/c/d */
    trie_acquire_lock(t, "/a/b/c/d", 10, true);
    
    /* Parent /a has intent_count = 1 */
    trie_node_t *node_a = trie_traverse(t, "/a", false, NULL);
    assert(node_a->intent_count == 1);
    
    /* Waiter on /a */
    assert(trie_acquire_lock(t, "/a", 11, true) == 1);
    
    /* Release /a/b/c/d */
    assert(trie_release_lock(t, "/a/b/c/d", 10) == 11);
    
    /* Now /a should be owned by 11 and have intent_count = 0 */
    assert(node_a->owner_fd == 11);
    assert(node_a->intent_count == 0);
    
    trie_destroy(t);
    printf("PASS\n");
}

void test_cleanup_massive_multi_node_wakeups() {
    printf("Testing cleanup with massive multi-node wakeups (100+ nodes)...\n");
    trie_t *t = trie_create();
    
    /* 10 owns 100 resources */
    for (int i = 0; i < 100; i++) {
        char path[32];
        sprintf(path, "/res/%d", i);
        trie_acquire_lock(t, path, 10, true);
    }
    
    /* Each resource has one waiter */
    for (int i = 0; i < 100; i++) {
        char path[32];
        sprintf(path, "/res/%d", i);
        assert(trie_acquire_lock(t, path, 100 + i, true) == 1);
    }
    
    /* Cleanup 10. Should wake up 100 people. 
       We provide a small buffer to see if it overflows or just caps. */
    int wakeup[16];
    char *w_paths[16];
    size_t count = trie_cleanup_fd(t, 10, wakeup, w_paths, 16);
    for (size_t i = 0; i < count; i++) free(w_paths[i]);
    
    /* The current implementation caps at 16, but we want to make sure it doesn't crash. */
    printf(" - Woke up %zu waiters (cap 16)\n", count);
    assert(count <= 16);
    
    /* Now verify that the remaining 84 were ALSO granted the lock in the trie 
       (even if they weren't returned in the wakeup array).
       Actually, if they are granted the lock, trie_get_owned_count(t, 100+i) should be 1. */
    for (int i = 0; i < 100; i++) {
        assert(trie_get_owned_count(t, 100 + i) == 1);
    }
    
    trie_destroy(t);
    printf("PASS\n");
}

int main() {
    test_basic_locking();
    test_hierarchical_locking();
    test_cleanup();
    test_deep_hierarchy();
    test_multi_waiter_queue();
    test_intent_lock_safety();
    test_edge_cases();
    test_path_normalization();
    test_multiple_blockers();
    test_waiter_abandonment();
    test_complex_tree();
    test_path_normalization_extreme();
    test_intent_lock_starvation();
    test_path_traversal_above_root();
    test_waiter_reordering();
    test_intent_churn();
    test_massive_waiter_queue();
    test_cleanup_recursive_intent();
    test_oversized_path();
    test_deep_intent_hierarchy_complex();
    test_path_segment_limit();
    test_cleanup_massive_wakeups();
    test_deep_recursive_parent_wakeup();
    test_cleanup_multi_branch_wakeup();
    test_breadth_stress();
    test_denied_on_ancestor_lock();
    test_cleanup_mixed_state();
    test_cascading_intent_decrement();
    test_cleanup_massive_multi_node_wakeups();
    printf("All 29 Trie test suites passed!\n");
    return 0;
}
