#ifndef SAFETY_H
#define SAFETY_H

#include <stdbool.h>

#define SAFETY_MAX_PATTERNS 8   /* max patterns that can match per check */
#define SAFETY_REASON_MAX 32    /* max length of a reason string */

struct safety_result {
    bool blocked;
    int match_count;
    const char *reasons[SAFETY_MAX_PATTERNS]; /* pointers into static storage */
};

/* Check a command string against dangerous patterns.
 * Returns blocked=true if any pattern matches, with reasons populated. */
struct safety_result safety_check(const char *command);

#endif /* SAFETY_H */
