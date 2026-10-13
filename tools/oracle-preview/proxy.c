/* One HTTP/1.1 exchange: 127.0.0.1:8317 -> host.docker.internal:8317.
 * The caller reads until EOF, so the client socket is closed after one
 * complete response. Used only when the product image has no python3.
 */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <signal.h>
#include <errno.h>
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/time.h>

#define LIMIT (8 * 1024 * 1024)

static int set_timeout(int fd) {
    struct timeval tv;
    tv.tv_sec = 180;
    tv.tv_usec = 0;
    return setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
}

static int header_end(const unsigned char *buf, size_t len) {
    size_t i;
    for (i = 0; i + 3 < len; i++) {
        if (buf[i] == '\r' && buf[i + 1] == '\n' && buf[i + 2] == '\r' && buf[i + 3] == '\n') {
            return (int)i;
        }
    }
    return -1;
}

static int content_length(const unsigned char *head, int head_len, int *chunked) {
    int i = 0;
    *chunked = 0;
    while (i < head_len) {
        int start = i;
        int colon = -1;
        while (i < head_len && !(head[i] == '\r' && i + 1 < head_len && head[i + 1] == '\n')) {
            if (colon < 0 && head[i] == ':') {
                colon = i;
            }
            i++;
        }
        if (colon > start) {
            int name_len = colon - start;
            const char *value = (const char *)head + colon + 1;
            int value_len = i - (colon + 1);
            while (value_len > 0 && (*value == ' ' || *value == '\t')) {
                value++;
                value_len--;
            }
            if (name_len == 14 && strncasecmp((const char *)head + start, "content-length", 14) == 0) {
                char tmp[32];
                char *end = NULL;
                long parsed;
                if (value_len <= 0 || value_len >= (int)sizeof tmp) {
                    return -2;
                }
                memcpy(tmp, value, (size_t)value_len);
                tmp[value_len] = 0;
                parsed = strtol(tmp, &end, 10);
                if (end == tmp || *end != 0 || parsed < 0 || parsed > LIMIT) {
                    return -2;
                }
                return (int)parsed;
            }
            if (name_len == 17 && strncasecmp((const char *)head + start, "transfer-encoding", 17) == 0) {
                if (value_len >= 7 && strncasecmp(value, "chunked", 7) == 0) {
                    *chunked = 1;
                }
            }
        }
        if (i + 1 < head_len && head[i] == '\r') {
            i += 2;
        } else {
            break;
        }
    }
    return -1;
}

static int read_some(int fd, unsigned char *buf, size_t *len, size_t cap) {
    ssize_t n;
    if (*len >= cap) {
        return -1;
    }
    n = read(fd, buf + *len, cap - *len);
    if (n <= 0) {
        return -1;
    }
    *len += (size_t)n;
    return 0;
}

static int read_message(int fd, unsigned char **out, size_t *out_len) {
    unsigned char *buf = malloc(LIMIT + 4);
    size_t len = 0;
    int split;
    int length;
    int chunked = 0;
    if (buf == NULL) {
        return -1;
    }
    split = -1;
    while (split < 0) {
        if (read_some(fd, buf, &len, LIMIT) != 0) {
            free(buf);
            return -1;
        }
        split = header_end(buf, len);
    }
    length = content_length(buf, split, &chunked);
    if (length == -2) {
        free(buf);
        return -1;
    }
    if (length >= 0) {
        size_t need = (size_t)split + 4 + (size_t)length;
        while (len < need) {
            if (read_some(fd, buf, &len, need) != 0) {
                free(buf);
                return -1;
            }
        }
        *out = buf;
        *out_len = need;
        return 0;
    }
    if (chunked) {
        while (len < 5 || memmem(buf, len, "\r\n0\r\n\r\n", 7) == NULL) {
            if (read_some(fd, buf, &len, LIMIT) != 0) {
                break;
            }
        }
    } else {
        while (read_some(fd, buf, &len, LIMIT) == 0) {
        }
    }
    if (len == 0 || header_end(buf, len) < 0) {
        free(buf);
        return -1;
    }
    *out = buf;
    *out_len = len;
    return 0;
}

static int write_all(int fd, const unsigned char *buf, size_t len) {
    size_t off = 0;
    while (off < len) {
        ssize_t n = write(fd, buf + off, len - off);
        if (n <= 0) {
            return -1;
        }
        off += (size_t)n;
    }
    return 0;
}

static void handle(int client) {
    struct addrinfo hints;
    struct addrinfo *res = NULL;
    unsigned char *request = NULL;
    unsigned char *response = NULL;
    size_t request_len = 0;
    size_t response_len = 0;
    int remote = -1;
    set_timeout(client);
    memset(&hints, 0, sizeof hints);
    hints.ai_socktype = SOCK_STREAM;
    if (read_message(client, &request, &request_len) != 0) {
        goto done;
    }
    if (getaddrinfo("host.docker.internal", "8317", &hints, &res) != 0) {
        goto done;
    }
    remote = socket(res->ai_family, res->ai_socktype, res->ai_protocol);
    if (remote < 0 || connect(remote, res->ai_addr, res->ai_addrlen) != 0) {
        goto done;
    }
    set_timeout(remote);
    if (write_all(remote, request, request_len) != 0) {
        goto done;
    }
    if (read_message(remote, &response, &response_len) != 0) {
        goto done;
    }
    write_all(client, response, response_len);
done:
    free(request);
    free(response);
    if (res != NULL) {
        freeaddrinfo(res);
    }
    if (remote >= 0) {
        close(remote);
    }
    close(client);
}

int main(void) {
    int server = socket(AF_INET, SOCK_STREAM, 0);
    signal(SIGCHLD, SIG_IGN);
    int one = 1;
    struct sockaddr_in addr;
    if (server < 0) {
        return 1;
    }
    setsockopt(server, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    memset(&addr, 0, sizeof addr);
    addr.sin_family = AF_INET;
    addr.sin_port = htons(8317);
    addr.sin_addr.s_addr = htonl(0x7f000001);
    if (bind(server, (struct sockaddr *)&addr, sizeof addr) != 0) {
        return 1;
    }
    if (listen(server, 32) != 0) {
        return 1;
    }
    {
        FILE *ready = fopen("/work/proxy-ready", "w");
        if (ready == NULL) {
            return 1;
        }
        fputs("ready\n", ready);
        fclose(ready);
    }
    for (;;) {
        int client = accept(server, NULL, NULL);
        if (client < 0) {
            if (errno == EINTR) {
                continue;
            }
            return 1;
        }
        if (fork() == 0) {
            close(server);
            handle(client);
            _exit(0);
        }
        close(client);
    }
}
