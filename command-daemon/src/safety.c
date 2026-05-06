#include "safety.h"
#include <string.h>

struct pattern {
    const char *substr;     /* simple substring match */
    const char *reason;     /* machine-readable reason tag */
};

static const struct pattern dangerous[] = {
    /* Recursive deletes targeting root or home */
    {"rm -rf /",        "recursive_delete"},
    {"rm -rf /*",       "recursive_delete"},
    {"rm -rf ~",        "recursive_delete"},
    {"rm -rf $HOME",    "recursive_delete"},
    {"rm -rf /*.",      "recursive_delete"},

    /* Reverse shells */
    {"/dev/tcp",        "reverse_shell"},
    {"nc -e /bin",      "reverse_shell"},
    {"ncat -e /bin",    "reverse_shell"},
    {"socat exec",      "reverse_shell"},
    {"bash -i >&",      "reverse_shell"},

    /* Fork bombs */
    {":(){ :|:& };:",   "fork_bomb"},
    {":(){:|:&};:",     "fork_bomb"},

    /* Raw device writes */
    {"dd if=",          NULL},   /* dd is conditional — only block with of=/dev */
    {"mkfs",            "filesystem_format"},

    /* Permission bypass */
    {"chmod -R 777 /",  "permission_bypass"},
    {"chmod 777 /etc/passwd", "permission_bypass"},

    /* Pipe untrusted content to shell */
    {"curl | sh",       "pipe_to_shell"},
    {"curl | bash",     "pipe_to_shell"},
    {"wget | sh",       "pipe_to_shell"},
    {"wget | bash",     "pipe_to_shell"},

    {NULL, NULL}  /* sentinel */
};

/* Special cases that need compound checks */
static bool is_dd_to_device(const char *cmd) {
    const char *dd = strstr(cmd, "dd ");
    if (!dd) dd = strstr(cmd, "dd=");
    if (!dd) return false;
    return strstr(dd, "of=/dev") != NULL;
}

struct safety_result safety_check(const char *command) {
    struct safety_result r = { .blocked = false, .match_count = 0 };

    /* Track which reasons we've already added (dedup) */
    const char *seen[SAFETY_MAX_PATTERNS];
    int seen_count = 0;

    for (int i = 0; dangerous[i].substr != NULL; i++) {
        if (!strstr(command, dangerous[i].substr))
            continue;

        /* dd is only dangerous when writing to /dev */
        if (strcmp(dangerous[i].substr, "dd if=") == 0) {
            if (!is_dd_to_device(command)) continue;
            const char *reason = "raw_device_write";
            bool dup = false;
            for (int s = 0; s < seen_count; s++)
                if (strcmp(seen[s], reason) == 0) { dup = true; break; }
            if (!dup && r.match_count < SAFETY_MAX_PATTERNS) {
                seen[seen_count++] = reason;
                r.reasons[r.match_count++] = reason;
            }
            r.blocked = true;
            continue;
        }

        /* Skip entries with no reason (dd base case) */
        if (!dangerous[i].reason) continue;

        /* Dedup: skip if we already have this reason */
        bool dup = false;
        for (int s = 0; s < seen_count; s++)
            if (strcmp(seen[s], dangerous[i].reason) == 0) { dup = true; break; }
        if (dup) continue;

        if (r.match_count < SAFETY_MAX_PATTERNS) {
            seen[seen_count++] = dangerous[i].reason;
            r.reasons[r.match_count++] = dangerous[i].reason;
        }
        r.blocked = true;
    }

    return r;
}
