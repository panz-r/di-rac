#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include "protocol.h"
#include "session.h"

#define MAX_LINE 65536

int main(int argc, char *argv[]) {
    char workspace_root[4096] = "";

    /* Parse --workspace-root <path> */
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--workspace-root") == 0 && i + 1 < argc) {
            strncpy(workspace_root, argv[i + 1], sizeof(workspace_root) - 1);
            i++;
        }
    }
    if (!workspace_root[0]) {
        if (getcwd(workspace_root, sizeof(workspace_root)) == NULL) {
            strcpy(workspace_root, "/");
        }
    }

    SessionStore store;
    session_store_init(&store);

    /* Signal readiness */
    fprintf(stderr, "ready\n");
    fflush(stderr);

    /* Main loop: read JSON lines from stdin */
    char *line = NULL;
    size_t line_cap = 0;
    FILE *in = stdin;

    /* Use getline for robust line reading */
    while (1) {
        ssize_t len = getline(&line, &line_cap, in);
        if (len < 0) break; /* EOF or error */
        if (len == 0) continue;

        /* Strip trailing newline */
        if (line[len - 1] == '\n') line[len - 1] = '\0';
        if (len > 1 && line[len - 2] == '\r') line[len - 2] = '\0';

        if (line[0] == '\0') continue;

        proto_handle_request(line, &store, workspace_root);
    }

    free(line);
    return 0;
}
