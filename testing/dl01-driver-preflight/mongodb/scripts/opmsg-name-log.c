/* Log OP_MSG command names the probe writes. Values stay off the log. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/uio.h>
#include <unistd.h>

#define SLOT_COUNT 16
#define SLOT_CAP (1024 * 1024)

static const char *ALLOWED[] = {
    "hello", "ismaster", "isMaster", "saslStart", "saslContinue", "find", "getMore",
    "killCursors", "endSessions", "listCollections", "listIndexes", "collStats",
    "connectionStatus", "aggregate", "ping", "buildInfo",
};

static int logfd = -1;
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static ssize_t (*real_write)(int, const void *, size_t);
__attribute__((unused)) static ssize_t (*real_send)(int, const void *, size_t, int);
__attribute__((unused)) static ssize_t (*real_writev)(int, const struct iovec *, int);
__attribute__((unused)) static ssize_t (*real_sendmsg)(int, const struct msghdr *, int);

struct Slot {
    int fd;
    int used;
    dev_t dev;
    ino_t ino;
    unsigned char *data;
    size_t len;
};

static struct Slot slots[SLOT_COUNT];

static uint32_t read_u32(const unsigned char *p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static int allowed_name(const char *name) {
    size_t i;
    for (i = 0; i < sizeof ALLOWED / sizeof ALLOWED[0]; i++) {
        if (strcmp(name, ALLOWED[i]) == 0) {
            return 1;
        }
    }
    return 0;
}

static int skip_cstring(const unsigned char *doc, size_t doclen, size_t *off) {
    size_t end = *off;
    while (end < doclen && doc[end] != 0) {
        end++;
    }
    if (end >= doclen) {
        return 0;
    }
    *off = end + 1;
    return 1;
}

static int read_cstring(const unsigned char *doc, size_t doclen, size_t *off, char *out, size_t outlen) {
    size_t start = *off;
    size_t end = start;
    if (start >= doclen || outlen == 0) {
        return 0;
    }
    while (end < doclen && doc[end] != 0) {
        end++;
    }
    if (end >= doclen || end - start >= outlen) {
        return 0;
    }
    memcpy(out, doc + start, end - start);
    out[end - start] = 0;
    *off = end + 1;
    return 1;
}

static int skip_value(const unsigned char *doc, size_t doclen, size_t *off, unsigned char type) {
    uint32_t n;
    size_t at = *off;
    if (type == 0x01 || type == 0x09 || type == 0x11) {
        n = 8;
    } else if (type == 0x10) {
        n = 4;
    } else if (type == 0x12) {
        n = 8;
    } else if (type == 0x13) {
        n = 16;
    } else if (type == 0x07) {
        n = 12;
    } else if (type == 0x08) {
        n = 1;
    } else if (type == 0x0A || type == 0xFF || type == 0x7F || type == 0x06) {
        n = 0;
    } else if (type == 0x02 || type == 0x0D || type == 0x0E) {
        if (at + 4 > doclen) {
            return 0;
        }
        n = read_u32(doc + at);
        if (n > doclen || at + 4 + n > doclen) {
            return 0;
        }
        *off = at + 4 + n;
        return 1;
    } else if (type == 0x03 || type == 0x04) {
        if (at + 4 > doclen) {
            return 0;
        }
        n = read_u32(doc + at);
        if (n < 5 || at + n > doclen) {
            return 0;
        }
        *off = at + n;
        return 1;
    } else if (type == 0x05) {
        if (at + 5 > doclen) {
            return 0;
        }
        n = read_u32(doc + at);
        if (at + 5 + n > doclen) {
            return 0;
        }
        *off = at + 5 + n;
        return 1;
    } else if (type == 0x0B) {
        return skip_cstring(doc, doclen, off) && skip_cstring(doc, doclen, off);
    } else if (type == 0x0C) {
        if (at + 4 > doclen) {
            return 0;
        }
        n = read_u32(doc + at);
        if (at + 4 + n + 12 > doclen) {
            return 0;
        }
        *off = at + 4 + n + 12;
        return 1;
    } else if (type == 0x0F) {
        if (at + 4 > doclen) {
            return 0;
        }
        n = read_u32(doc + at);
        if (n < 5 || at + n > doclen) {
            return 0;
        }
        *off = at + n;
        return 1;
    } else {
        return 0;
    }
    if (at + n > doclen) {
        return 0;
    }
    *off = at + n;
    return 1;
}

/* First key of a nested document, compared without copying the value. */
static int first_key_is(const unsigned char *doc, uint32_t doclen, const char *want) {
    size_t off = 4;
    unsigned char type;
    char key[64];
    if (doclen < 5) {
        return 0;
    }
    type = doc[off++];
    if (type == 0 || !read_cstring(doc, doclen, &off, key, sizeof key)) {
        return 0;
    }
    return strcmp(key, want) == 0;
}

/* Returns 1 when a command name was taken from the first BSON key.
   nested_sasl is 1 when speculativeAuthenticate's first key is saslStart.
   The nested document is not copied. */
static int command_of(const unsigned char *msg, size_t msglen, char *name, size_t namelen, int *speculative, int *nested_sasl) {
    uint32_t blen;
    size_t off;
    unsigned char type;
    const unsigned char *doc;
    *speculative = 0;
    *nested_sasl = 0;
    name[0] = 0;
    if (msglen < 26 || read_u32(msg + 12) != 2013 || msg[20] != 0) {
        return 0;
    }
    doc = msg + 21;
    if (msglen - 21 < 5) {
        return 0;
    }
    blen = read_u32(doc);
    if (blen < 5 || (size_t)blen > msglen - 21) {
        return 0;
    }
    off = 4;
    type = doc[off++];
    if (type == 0 || !read_cstring(doc, blen, &off, name, namelen)) {
        name[0] = 0;
        return 0;
    }
    if (!skip_value(doc, blen, &off, type)) {
        return 1;
    }
    while (off < blen) {
        char key[64];
        type = doc[off++];
        if (type == 0) {
            break;
        }
        if (!read_cstring(doc, blen, &off, key, sizeof key)) {
            break;
        }
        if (strcmp(key, "speculativeAuthenticate") == 0) {
            uint32_t nlen;
            *speculative = 1;
            if (type == 0x03 && off + 4 <= blen) {
                nlen = read_u32(doc + off);
                if (nlen >= 5 && off + nlen <= blen && first_key_is(doc + off, nlen, "saslStart")) {
                    *nested_sasl = 1;
                }
            }
        }
        if (!skip_value(doc, blen, &off, type)) {
            break;
        }
    }
    return 1;
}

static void emit_line(const char *command, int speculative) {
    char line[160];
    int n;
    if (logfd < 0 || real_write == NULL) {
        return;
    }
    n = snprintf(line, sizeof line, "{\"command\":\"%s\",\"speculative_authenticate\":%s}\n",
                 command, speculative ? "true" : "false");
    if (n > 0 && (size_t)n < sizeof line) {
        real_write(logfd, line, (size_t)n);
    }
}

static struct Slot *slot_for(int fd) {
    size_t i;
    struct stat st;
    if (fstat(fd, &st) != 0 || !S_ISSOCK(st.st_mode)) {
        for (i = 0; i < SLOT_COUNT; i++) {
            if (slots[i].used && slots[i].fd == fd) {
                slots[i].used = 0;
                slots[i].len = 0;
            }
        }
        return NULL;
    }
    for (i = 0; i < SLOT_COUNT; i++) {
        if (slots[i].used && slots[i].fd == fd && slots[i].dev == st.st_dev && slots[i].ino == st.st_ino) {
            return &slots[i];
        }
    }
    for (i = 0; i < SLOT_COUNT; i++) {
        if (!slots[i].used || slots[i].fd == fd) {
            slots[i].used = 1;
            slots[i].fd = fd;
            slots[i].dev = st.st_dev;
            slots[i].ino = st.st_ino;
            slots[i].len = 0;
            if (slots[i].data == NULL) {
                slots[i].data = malloc(SLOT_CAP);
                if (slots[i].data == NULL) {
                    slots[i].used = 0;
                    return NULL;
                }
            }
            return &slots[i];
        }
    }
    emit_line("oversize", 0);
    return NULL;
}

static void consume(struct Slot *slot) {
    while (slot->len >= 16) {
        uint32_t msglen = read_u32(slot->data);
        char name[64];
        int speculative = 0;
        int nested_sasl = 0;
        if (msglen < 16 || msglen > SLOT_CAP) {
            emit_line("unparsed", 0);
            slot->len = 0;
            return;
        }
        if (slot->len < msglen) {
            return;
        }
        if (command_of(slot->data, msglen, name, sizeof name, &speculative, &nested_sasl)) {
            if (allowed_name(name)) {
                emit_line(name, speculative);
                if (nested_sasl) {
                    emit_line("saslStart", 1);
                }
            } else {
                emit_line("unlisted", 0);
            }
        }
        memmove(slot->data, slot->data + msglen, slot->len - msglen);
        slot->len -= msglen;
    }
}

__attribute__((unused)) static void feed(int fd, const void *buf, size_t n) {
    struct Slot *slot;
    if (n == 0 || logfd < 0 || fd == logfd) {
        return;
    }
    pthread_mutex_lock(&mu);
    slot = slot_for(fd);
    if (slot != NULL) {
        if (slot->len + n > SLOT_CAP) {
            emit_line("oversize", 0);
            slot->len = 0;
        } else {
            memcpy(slot->data + slot->len, buf, n);
            slot->len += n;
            consume(slot);
        }
    }
    pthread_mutex_unlock(&mu);
}

#ifdef OO_OPMSG_TEST
static unsigned char *build_doc(size_t *outlen) {
    /* { isMaster: 1, helloOk: true, speculativeAuthenticate: { saslStart: 1 } } */
    const char *nested_key = "saslStart";
    const char *k0 = "isMaster";
    const char *k1 = "helloOk";
    const char *k2 = "speculativeAuthenticate";
    size_t nested_len = 4 + 1 + strlen(nested_key) + 1 + 4 + 1;
    size_t len = 4
        + 1 + strlen(k0) + 1 + 4
        + 1 + strlen(k1) + 1 + 1
        + 1 + strlen(k2) + 1 + nested_len
        + 1;
    unsigned char *doc = calloc(1, len);
    size_t at = 0;
    uint32_t nlen = (uint32_t)nested_len;
    uint32_t dlen = (uint32_t)len;
    uint32_t one = 1;
    if (doc == NULL) {
        return NULL;
    }
    memcpy(doc + at, &dlen, 4);
    at += 4;
    doc[at++] = 0x10;
    memcpy(doc + at, k0, strlen(k0) + 1);
    at += strlen(k0) + 1;
    memcpy(doc + at, &one, 4);
    at += 4;
    doc[at++] = 0x08;
    memcpy(doc + at, k1, strlen(k1) + 1);
    at += strlen(k1) + 1;
    doc[at++] = 1;
    doc[at++] = 0x03;
    memcpy(doc + at, k2, strlen(k2) + 1);
    at += strlen(k2) + 1;
    memcpy(doc + at, &nlen, 4);
    at += 4;
    doc[at++] = 0x10;
    memcpy(doc + at, nested_key, strlen(nested_key) + 1);
    at += strlen(nested_key) + 1;
    memcpy(doc + at, &one, 4);
    at += 4;
    doc[at++] = 0;
    doc[at++] = 0;
    *outlen = len;
    return doc;
}

int main(void) {
    size_t doclen = 0;
    unsigned char *doc = build_doc(&doclen);
    unsigned char msg[512];
    uint32_t msglen;
    char name[64];
    int speculative = 0;
    int nested_sasl = 0;
    int ok;
    if (doc == NULL) {
        return 2;
    }
    msglen = (uint32_t)(16 + 4 + 1 + doclen);
    memset(msg, 0, sizeof msg);
    memcpy(msg, &msglen, 4);
    msg[12] = (unsigned char)(2013 & 0xff);
    msg[13] = (unsigned char)((2013 >> 8) & 0xff);
    msg[20] = 0;
    memcpy(msg + 21, doc, doclen);
    ok = command_of(msg, msglen, name, sizeof name, &speculative, &nested_sasl);
    free(doc);
    if (!ok || strcmp(name, "isMaster") != 0 || !speculative || !nested_sasl || !allowed_name(name)) {
        fprintf(stderr, "opmsg self-check failed name=%s spec=%d nested=%d ok=%d\n", name, speculative, nested_sasl, ok);
        return 1;
    }
    if (allowed_name("insert") || allowed_name("saslStartPayload")) {
        fprintf(stderr, "allow-list accepted a write or a payload\n");
        return 1;
    }
    printf("opmsg self-check ok\n");
    return 0;
}
#else
__attribute__((constructor)) static void oo_opmsg_init(void) {
    const char *path = getenv("OO_W141_OPMSG_LOG");
    real_write = dlsym(RTLD_NEXT, "write");
    real_send = dlsym(RTLD_NEXT, "send");
    real_writev = dlsym(RTLD_NEXT, "writev");
    real_sendmsg = dlsym(RTLD_NEXT, "sendmsg");
    if (path != NULL && path[0] != 0 && real_write != NULL) {
        logfd = open(path, O_CREAT | O_APPEND | O_WRONLY | O_CLOEXEC, 0644);
    }
}

ssize_t write(int fd, const void *buf, size_t count) {
    ssize_t n;
    if (real_write == NULL) {
        real_write = dlsym(RTLD_NEXT, "write");
    }
    n = real_write(fd, buf, count);
    if (n > 0) {
        feed(fd, buf, (size_t)n);
    }
    return n;
}

ssize_t send(int fd, const void *buf, size_t count, int flags) {
    ssize_t n;
    if (real_send == NULL) {
        real_send = dlsym(RTLD_NEXT, "send");
    }
    n = real_send(fd, buf, count, flags);
    if (n > 0) {
        feed(fd, buf, (size_t)n);
    }
    return n;
}

ssize_t writev(int fd, const struct iovec *iov, int iovcnt) {
    ssize_t n;
    int i;
    if (real_writev == NULL) {
        real_writev = dlsym(RTLD_NEXT, "writev");
    }
    n = real_writev(fd, iov, iovcnt);
    if (n > 0) {
        size_t left = (size_t)n;
        for (i = 0; i < iovcnt && left > 0; i++) {
            size_t take = iov[i].iov_len < left ? iov[i].iov_len : left;
            feed(fd, iov[i].iov_base, take);
            left -= take;
        }
    }
    return n;
}

ssize_t sendmsg(int fd, const struct msghdr *msg, int flags) {
    ssize_t n;
    if (real_sendmsg == NULL) {
        real_sendmsg = dlsym(RTLD_NEXT, "sendmsg");
    }
    n = real_sendmsg(fd, msg, flags);
    if (n > 0 && msg != NULL) {
        size_t left = (size_t)n;
        size_t count = (size_t)msg->msg_iovlen;
        size_t index;
        for (index = 0; index < count && left > 0; index++) {
            size_t take = msg->msg_iov[index].iov_len < left ? msg->msg_iov[index].iov_len : left;
            feed(fd, msg->msg_iov[index].iov_base, take);
            left -= take;
        }
    }
    return n;
}
#endif
