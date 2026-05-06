#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <assert.h>
#include <errno.h>
#include <poll.h>

#define SOCKET_PATH "/tmp/di-vrr-coord.sock"

int connect_to_broker() {
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, SOCKET_PATH, sizeof(addr.sun_path) - 1);
    
    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) == -1) {
        perror("connect");
        return -1;
    }
    return fd;
}

void send_cmd(int fd, const char *json) {
    if (write(fd, json, strlen(json)) < 0) perror("write");
}

int read_resp_all(int fd, char *buf, size_t len) {
    struct pollfd pfd = { .fd = fd, .events = POLLIN };
    int ret = poll(&pfd, 1, 1000); /* 1s timeout */
    if (ret <= 0) {
        buf[0] = 0;
        return 0;
    }
    ssize_t n = read(fd, buf, len - 1);
    if (n > 0) {
        buf[n] = 0;
        printf("   < RESP: %s", buf);
        return (int)n;
    } else {
        buf[0] = 0;
        return 0;
    }
}

void test_robustness() {
    printf("Testing broker robustness...\n");
    int c = connect_to_broker();
    assert(c != -1);
    char resp[8192];

    /* 1. Malformed JSON - incomplete */
    printf(" - Sending malformed JSON (incomplete)\n");
    send_cmd(c, "{\"method\": \"acquire\", \"path\": "); 
    read_resp_all(c, resp, sizeof(resp));
    
    send_cmd(c, "\"/robust_incomplete\", \"wait\": true}");
    read_resp_all(c, resp, sizeof(resp));
    assert(strstr(resp, "\"status\": \"ok\"") != NULL);

    /* 2. Multiple commands in one packet */
    printf(" - Sending multiple commands in one packet\n");
    send_cmd(c, "{\"method\": \"release\", \"path\": \"/robust_incomplete\"}{\"method\": \"acquire\", \"path\": \"/robust2\"}");
    
    int ok_count = 0;
    for (int i = 0; i < 5; i++) {
        read_resp_all(c, resp, sizeof(resp));
        char *p = resp;
        while ((p = strstr(p, "\"status\": \"ok\""))) {
            ok_count++;
            p++;
        }
        if (ok_count >= 2) break;
        usleep(100000);
    }
    assert(ok_count == 2);

    close(c);
    printf("PASS\n");
}

void test_fragmented_json() {
    printf("Testing fragmented JSON parsing...\n");
    int c = connect_to_broker();
    assert(c != -1);
    char resp[4096];

    const char *json = "{\"method\": \"acquire\", \"path\": \"/frag\", \"wait\": true}";
    size_t len = strlen(json);

    /* Send JSON one byte at a time */
    for (size_t i = 0; i < len; i++) {
        if (write(c, &json[i], 1) < 0) perror("write");
        usleep(1000); 
    }

    read_resp_all(c, resp, sizeof(resp));
    assert(strstr(resp, "\"status\": \"ok\"") != NULL);

    close(c);
    printf("PASS\n");
}

void test_oversized_payload() {
    printf("Testing oversized JSON payload...\n");
    int c = connect_to_broker();
    assert(c != -1);
    char resp[16384];
    
    char long_path[5000];
    memset(long_path, 'b', sizeof(long_path));
    long_path[0] = '/';
    long_path[4999] = '\0';

    char *json_str;
    asprintf(&json_str, "{\"method\": \"acquire\", \"path\": \"%s\", \"wait\": true}", long_path);
    send_cmd(c, json_str);
    
    read_resp_all(c, resp, sizeof(resp));
    assert(strstr(resp, "\"status\": \"ok\"") != NULL);

    free(json_str);
    close(c);
    printf("PASS\n");
}

void test_garbage_skip() {
    printf("Testing garbage skip between JSON...\n");
    int c = connect_to_broker();
    assert(c != -1);
    char resp[1024];

    send_cmd(c, "{\"method\": \"acquire\", \"path\": \"/g1\", \"wait\": true}  !!! GARBAGE !!! {\"method\": \"acquire\", \"path\": \"/g2\", \"wait\": true}");

    int ok_count = 0;
    for (int i = 0; i < 5; i++) {
        read_resp_all(c, resp, sizeof(resp));
        char *p = resp;
        while ((p = strstr(p, "\"status\": \"ok\""))) {
            ok_count++;
            p++;
        }
        if (ok_count >= 2) break;
        usleep(100000);
    }
    assert(ok_count == 2);

    close(c);
    printf("PASS\n");
}

void test_massive_wakeups() {
    printf("Testing massive wakeups (100 waiters)...\n");
    int owner = connect_to_broker();
    assert(owner != -1);

    send_cmd(owner, "{\"method\": \"acquire\", \"path\": \"/heavy\", \"wait\": true}");
    char resp[1024];
    read_resp_all(owner, resp, sizeof(resp));

    int waiters[100];
    for (int i = 0; i < 100; i++) {
        waiters[i] = connect_to_broker();
        send_cmd(waiters[i], "{\"method\": \"acquire\", \"path\": \"/heavy\", \"wait\": true}");
        read_resp_all(waiters[i], resp, sizeof(resp));
        assert(strstr(resp, "\"status\": \"waiting\"") != NULL);
    }

    /* Disconnect owner. This should trigger wakeups. */
    close(owner);

    /* Direct waiter for /heavy should be woken up. */
    int woken_count = 0;
    for (int i = 0; i < 100; i++) {
        struct pollfd pfd = { .fd = waiters[i], .events = POLLIN };
        if (poll(&pfd, 1, 500) > 0) {
            read_resp_all(waiters[i], resp, sizeof(resp));
            if (strstr(resp, "\"status\": \"granted\"")) {
                woken_count++;
            }
        }
    }
    printf(" - Woken up: %d/100\n", woken_count);
    assert(woken_count == 1);

    for (int i = 0; i < 100; i++) close(waiters[i]);
    printf("PASS\n");
}

void test_disconnect_fragmented() {
    printf("Testing disconnect with fragmented buffer...\n");
    int c = connect_to_broker();

    /* Send partial JSON */
    send_cmd(c, "{\"method\": \"acquire\", \"path\": ");

    /* Disconnect immediately */
    close(c);

    /* Daemon should clean up without crashing */
    printf("PASS\n");
}

int main() {
    setvbuf(stdout, NULL, _IONBF, 0);
    printf("Starting Broker Integration Test...\n");
    
    test_robustness();
    test_fragmented_json();
    test_oversized_payload();
    test_garbage_skip();
    test_massive_wakeups();
    test_disconnect_fragmented();

    int c1 = connect_to_broker();
    int c2 = connect_to_broker();
    assert(c1 != -1 && c2 != -1);

    char resp[4096];

    /* Basic ACQ/REL Flow */
    printf("Testing basic ACQ/REL flow...\n");
    send_cmd(c1, "{\"method\": \"acquire\", \"path\": \"/test\", \"wait\": true}");
    read_resp_all(c1, resp, sizeof(resp));
    assert(strstr(resp, "\"status\": \"ok\"") != NULL);

    send_cmd(c2, "{\"method\": \"acquire\", \"path\": \"/test\", \"wait\": true}");
    read_resp_all(c2, resp, sizeof(resp));
    assert(strstr(resp, "\"status\": \"waiting\"") != NULL);

    send_cmd(c1, "{\"method\": \"release\", \"path\": \"/test\"}");
    read_resp_all(c1, resp, sizeof(resp));
    assert(strstr(resp, "\"status\": \"ok\"") != NULL);

    read_resp_all(c2, resp, sizeof(resp));
    assert(strstr(resp, "\"status\": \"granted\"") != NULL);

    close(c1);
    close(c2);

    printf("Broker Integration Test PASS\n");
    return 0;
}
