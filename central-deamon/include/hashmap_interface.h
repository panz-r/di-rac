#ifndef DI_VRR_HASHMAP_INTERFACE_H
#define DI_VRR_HASHMAP_INTERFACE_H

/* 
 * Abstract interface for the high-performance HashMap.
 * Replace with your project's layered design.
 */

typedef struct hash_map hash_map_t;

/* Stub functions - to be mapped to your implementation */
hash_map_t* hm_create(void);
void hm_destroy(hash_map_t *map);
void hm_set(hash_map_t *map, const char *key, void *value);
void* hm_get(hash_map_t *map, const char *key);
void hm_delete(hash_map_t *map, const char *key);

#endif /* DI_VRR_HASHMAP_INTERFACE_H */
