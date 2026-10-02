#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

static const char *base_name(const char *path) {
    const char *slash = strrchr(path, '/');
    return slash ? slash + 1 : path;
}

static int read_starttime(char *out, size_t cap) {
    FILE *file = fopen("/proc/self/stat", "r");
    char buf[1024];
    char *paren;
    char *save = NULL;
    char *tok;
    int index = 0;
    if (!file) {
        return -1;
    }
    if (!fgets(buf, sizeof buf, file)) {
        fclose(file);
        return -1;
    }
    fclose(file);
    paren = strrchr(buf, ')');
    if (!paren) {
        return -1;
    }
    tok = strtok_r(paren + 1, " \n", &save);
    while (tok) {
        if (index == 19) {
            snprintf(out, cap, "%s", tok);
            return 0;
        }
        tok = strtok_r(NULL, " \n", &save);
        index++;
    }
    return -1;
}

static void write_frame(uint16_t kind, const void *body, uint32_t len) {
    unsigned char hdr[12];
    memcpy(hdr, "P05P", 4);
    hdr[4] = 1;
    hdr[5] = 0;
    hdr[6] = (unsigned char)(kind & 0xff);
    hdr[7] = (unsigned char)((kind >> 8) & 0xff);
    hdr[8] = (unsigned char)(len & 0xff);
    hdr[9] = (unsigned char)((len >> 8) & 0xff);
    hdr[10] = (unsigned char)((len >> 16) & 0xff);
    hdr[11] = (unsigned char)((len >> 24) & 0xff);
    if (fwrite(hdr, 1, 12, stdout) != 12) {
        return;
    }
    if (len > 0 && body) {
        fwrite(body, 1, len, stdout);
    }
    fflush(stdout);
}

static void write_json(uint16_t kind, const char *json) {
    write_frame(kind, json, (uint32_t)strlen(json));
}

static void read_one(void) {
    unsigned char hdr[12];
    uint32_t len;
    size_t got = 0;
    char *buf;
    if (fread(hdr, 1, 12, stdin) != 12) {
        return;
    }
    len = (uint32_t)hdr[8] | ((uint32_t)hdr[9] << 8) | ((uint32_t)hdr[10] << 16) | ((uint32_t)hdr[11] << 24);
    if (len > 1024 * 1024) {
        return;
    }
    buf = len ? malloc(len) : NULL;
    while (buf && got < len) {
        size_t n = fread(buf + got, 1, len - got, stdin);
        if (n == 0) {
            break;
        }
        got += n;
    }
    free(buf);
}

static void hold_forever(void) {
    signal(SIGPIPE, SIG_IGN);
    for (;;) {
        pause();
    }
}

static void fork_hold(int do_setsid) {
    pid_t child = fork();
    FILE *file;
    if (child != 0) {
        return;
    }
    if (do_setsid) {
        setsid();
    }
    file = fopen("/probe/descendant.pid", "w");
    if (file) {
        fprintf(file, "%d\n", (int)getpid());
        fclose(file);
    }
    file = fopen("/probe/descendant.stat", "w");
    if (file) {
        fprintf(file, "pid=%d ppid=%d pgid=%d sid=%d setsid=%d\n",
                (int)getpid(), (int)getppid(), (int)getpgrp(), (int)getsid(0), do_setsid);
        fclose(file);
    }
    hold_forever();
}

static char *decision(const char *nonce, int pid, int allow, const char *code, int version, int oid, const char *sql) {
    char *buf = malloc(4096);
    snprintf(buf, 4096,
             "{\"syntax_allow\":%s,\"code\":\"%s\",\"parser_version\":%d,\"node_count\":1,\"max_depth\":1,"
             "\"original_projection_star\":false,\"executed_sql\":\"%s\",\"detail\":\"a076-probe\","
             "\"oid_acl_rls_proven\":%s,\"nonce\":\"%s\",\"pid\":%d,\"reaped\":false,"
             "\"workdir_removed\":false,\"starttime_matched\":false}",
             allow ? "true" : "false", code, version, sql, oid ? "true" : "false", nonce, pid);
    return buf;
}

static void note_caps(void) {
    FILE *in = fopen("/proc/self/status", "r");
    FILE *out = fopen("/probe/worker-caps.txt", "w");
    char line[512];
    if (!in || !out) {
        if (in) {
            fclose(in);
        }
        if (out) {
            fclose(out);
        }
        return;
    }
    while (fgets(line, sizeof line, in)) {
        if (strncmp(line, "Cap", 3) == 0 || strncmp(line, "NoNewPrivs", 10) == 0) {
            fputs(line, out);
        }
    }
    fclose(in);
    fclose(out);
}

static void list_fds(void) {
    FILE *out = fopen("/probe/inherit-fds.txt", "w");
    char path[64];
    char dest[512];
    int fd;
    if (!out) {
        return;
    }
    fprintf(out, "pid=%d cwd_fd_count_start\n", (int)getpid());
    for (fd = 0; fd < 64; fd++) {
        snprintf(path, sizeof path, "/proc/self/fd/%d", fd);
        ssize_t n = readlink(path, dest, sizeof dest - 1);
        if (n < 0) {
            continue;
        }
        dest[n] = 0;
        fprintf(out, "fd=%d %s\n", fd, dest);
    }
    fclose(out);
}

static void touch_outside(void) {
    FILE *out = fopen("/probe/outside-touch.txt", "w");
    FILE *pass = fopen("/etc/passwd", "r");
    FILE *src = fopen("/src/Cargo.toml", "r");
    char line[256];
    int sock;
    struct sockaddr_in addr;
    if (!out) {
        return;
    }
    if (pass && fgets(line, sizeof line, pass)) {
        fprintf(out, "passwd_readable=yes first=%s", line);
    } else {
        fprintf(out, "passwd_readable=no errno=%d\n", errno);
    }
    if (src && fgets(line, sizeof line, src)) {
        fprintf(out, "src_cargo_readable=yes first=%s", line);
    } else {
        fprintf(out, "src_cargo_readable=no errno=%d\n", errno);
    }
    sock = socket(AF_INET, SOCK_STREAM, 0);
    memset(&addr, 0, sizeof addr);
    addr.sin_family = AF_INET;
    addr.sin_port = htons(443);
    inet_pton(AF_INET, "1.1.1.1", &addr.sin_addr);
    if (sock >= 0 && connect(sock, (struct sockaddr *)&addr, sizeof addr) == 0) {
        fprintf(out, "tcp_1_1_1_1_443=connected\n");
    } else {
        fprintf(out, "tcp_1_1_1_1_443=fail errno=%d\n", errno);
    }
    if (sock >= 0) {
        close(sock);
    }
    fclose(out);
    if (pass) {
        fclose(pass);
    }
    if (src) {
        fclose(src);
    }
}

int main(int argc, char **argv) {
    const char *mode = base_name(argv[0]);
    const char *nonce = argc > 1 ? argv[1] : "";
    char start[64] = "0";
    char body[512];
    int pid = (int)getpid();
    signal(SIGPIPE, SIG_IGN);
    if (strcmp(mode, "premature") == 0) {
        return 0;
    }
    if (strcmp(mode, "silent-sleep") == 0) {
        hold_forever();
    }
    read_starttime(start, sizeof start);
    if (strcmp(mode, "spoof-pid") == 0) {
        snprintf(body, sizeof body, "{\"nonce\":\"%s\",\"pid\":1,\"starttime\":\"%s\"}", nonce, start);
    } else if (strcmp(mode, "spoof-start") == 0) {
        snprintf(body, sizeof body, "{\"nonce\":\"%s\",\"pid\":%d,\"starttime\":\"1\"}", nonce, pid);
    } else if (strcmp(mode, "spoof-nonce") == 0) {
        snprintf(body, sizeof body, "{\"nonce\":\"deadbeef\",\"pid\":%d,\"starttime\":\"%s\"}", pid, start);
    } else if (strcmp(mode, "bad-magic") == 0) {
        fwrite("XXXX", 1, 4, stdout);
        fflush(stdout);
        hold_forever();
    } else {
        snprintf(body, sizeof body, "{\"nonce\":\"%s\",\"pid\":%d,\"starttime\":\"%s\"}", nonce, pid, start);
    }
    if (strcmp(mode, "bad-magic") != 0) {
        write_json(1, body);
    }
    if (strcmp(mode, "partial") == 0) {
        fwrite("P05", 1, 3, stdout);
        fflush(stdout);
        hold_forever();
    }
    if (strcmp(mode, "stdout-junk") == 0) {
        char *junk = malloc(1000000);
        memset(junk, 'A', 1000000);
        fwrite(junk, 1, 1000000, stdout);
        fflush(stdout);
        free(junk);
        hold_forever();
    }
    if (strcmp(mode, "oversize-frame") == 0) {
        uint32_t len = (256 * 1024) + 1;
        unsigned char hdr[12];
        char *junk;
        read_one();
        memcpy(hdr, "P05P", 4);
        hdr[4] = 1;
        hdr[5] = 0;
        hdr[6] = 3;
        hdr[7] = 0;
        hdr[8] = (unsigned char)(len & 0xff);
        hdr[9] = (unsigned char)((len >> 8) & 0xff);
        hdr[10] = (unsigned char)((len >> 16) & 0xff);
        hdr[11] = (unsigned char)((len >> 24) & 0xff);
        fwrite(hdr, 1, 12, stdout);
        junk = malloc(70000);
        memset(junk, 'B', 70000);
        fwrite(junk, 1, 70000, stdout);
        fflush(stdout);
        free(junk);
        hold_forever();
    }
    if (strcmp(mode, "hold-setsid-noreply") == 0) {
        fork_hold(1);
        _exit(0);
    }
    if (strcmp(mode, "inherit-report") == 0 || strcmp(mode, "net-touch") == 0) {
        note_caps();
        list_fds();
        if (strcmp(mode, "net-touch") == 0) {
            touch_outside();
        }
        hold_forever();
    }
    read_one();
    if (strcmp(mode, "stderr-flood") == 0) {
        char *junk = malloc(2000000);
        char *json;
        memset(junk, 'E', 2000000);
        fwrite(junk, 1, 2000000, stderr);
        fflush(stderr);
        free(junk);
        json = decision(nonce, pid, 0, "INVALID_OUTPUT", 170007, 0, "");
        write_json(3, json);
        free(json);
        return 0;
    }
    if (strcmp(mode, "contradict") == 0) {
        char *json = decision(nonce, pid, 0, "ALLOW_SYNTAX", 170007, 0, "DROP TABLE analytics.orders");
        write_json(3, json);
        free(json);
        return 0;
    }
    if (strcmp(mode, "oid-true") == 0) {
        char *json = decision(nonce, pid, 1, "ALLOW_SYNTAX", 170007, 1, "SELECT secret_from_worker");
        write_json(3, json);
        free(json);
        return 0;
    }
    if (strcmp(mode, "version-server") == 0) {
        char *json = decision(nonce, pid, 1, "ALLOW_SYNTAX", 170010, 0, "SELECT 1");
        write_json(3, json);
        free(json);
        return 0;
    }
    if (strcmp(mode, "quick-deny") == 0) {
        char *json = decision(nonce, pid, 0, "INVALID_OUTPUT", 170007, 0, "");
        write_json(3, json);
        free(json);
        return 0;
    }
    if (strcmp(mode, "extra-frame") == 0) {
        char *json = decision(nonce, pid, 0, "INVALID_OUTPUT", 170007, 0, "");
        write_json(3, json);
        write_json(3, json);
        free(json);
        return 0;
    }
    if (strcmp(mode, "hold-setsid-after") == 0) {
        char *json = decision(nonce, pid, 0, "INVALID_OUTPUT", 170007, 0, "");
        write_json(3, json);
        free(json);
        fork_hold(1);
        _exit(0);
    }
    if (strcmp(mode, "hold-samegroup") == 0) {
        char *json = decision(nonce, pid, 0, "INVALID_OUTPUT", 170007, 0, "");
        write_json(3, json);
        free(json);
        fork_hold(0);
        _exit(0);
    }
    hold_forever();
}
