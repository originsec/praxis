#include "tiny.h"
#include "conn.h"

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if !defined(_WIN32)
  #include <sys/select.h>
#endif

#include "bearssl.h"

//
// Trust anchors are generated at build time by `brssl ta -q` from the
// system CA bundle (see Makefile target $(TA_FILE)). The generated
// file declares `static const br_x509_trust_anchor TAs[N]` plus
// `#define TAs_NUM N`, so it must be #include'd into a translation
// unit — here.
//

#include "trust_anchors.inc"

struct conn {
    int    fd;
    int    is_tls;

    //
    // Cancel pointer used by the low-level read callback.  Updated by
    // conn_read() / conn_write_all() before each call so callers can
    // pass a per-request cancel flag without rebinding the BearSSL I/O
    // context.
    //

    volatile int *cancel;

    //
    // TLS state, only used when is_tls.
    //

    br_ssl_client_context   *sc;
    br_x509_minimal_context *xc;
    br_sslio_context         ioc;
    unsigned char           *iobuf;
};

static int tcp_connect(const char *host, int port)
{
    char portstr[16];
    snprintf(portstr, sizeof(portstr), "%d", port);
    struct addrinfo hints = {0}, *res = NULL;
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    int rc = getaddrinfo(host, portstr, &hints, &res);
    if (rc != 0) {
        LOG_WARN("getaddrinfo %s: %s", host, gai_strerror(rc));
        return -1;
    }
    int fd = -1;
    for (struct addrinfo *ai = res; ai; ai = ai->ai_next) {
        fd = socket(ai->ai_family, ai->ai_socktype | SOCK_CLOEXEC, ai->ai_protocol);
        if (fd < 0) continue;
        if (connect(fd, ai->ai_addr, ai->ai_addrlen) == 0) break;
        close_sock(fd);
        fd = -1;
    }
    freeaddrinfo(res);
    if (fd < 0) return -1;
    int one = 1;
    setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
    return fd;
}

//
// Low-level TLS read/write callbacks. ctx is the conn_t, so the
// callbacks can honor the per-call cancel flag and run a periodic
// select() so cancellation is responsive even mid-handshake.
//

static int low_read_cb(void *ctx, unsigned char *data, size_t len)
{
    struct conn *c = ctx;
    while (1) {
        if (c->cancel && *c->cancel) return -1;
        fd_set rfds;
        FD_ZERO(&rfds);
        FD_SET(c->fd, &rfds);
        struct timeval tv = { 1, 0 };
        int s = select(c->fd + 1, &rfds, NULL, NULL, &tv);
        if (s < 0) { if (errno == EINTR) continue; return -1; }
        if (s == 0) continue;
        ssize_t r = recv(c->fd, (char *)data, len, 0);
        if (r < 0) { if (errno == EINTR) continue; return -1; }
        if (r == 0) return -1;
        return (int)r;
    }
}

static int low_write_cb(void *ctx, const unsigned char *data, size_t len)
{
    struct conn *c = ctx;
    while (1) {
        if (c->cancel && *c->cancel) return -1;
        ssize_t w = send(c->fd, (const char *)data, len, MSG_NOSIGNAL);
        if (w < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        if (w == 0) return -1;
        return (int)w;
    }
}

conn_t *conn_open(const char *host, int port, int use_tls)
{
    int fd = tcp_connect(host, port);
    if (fd < 0) return NULL;

    struct conn *c = calloc(1, sizeof(*c));
    if (!c) { close_sock(fd); return NULL; }
    c->fd = fd;
    c->is_tls = use_tls ? 1 : 0;

    if (!use_tls) return c;

    c->sc     = calloc(1, sizeof(*c->sc));
    c->xc     = calloc(1, sizeof(*c->xc));
    c->iobuf  = malloc(BR_SSL_BUFSIZE_BIDI);
    if (!c->sc || !c->xc || !c->iobuf) {
        conn_close(c);
        return NULL;
    }

    br_ssl_client_init_full(c->sc, c->xc, TAs, TAs_NUM);
    br_ssl_engine_set_buffer(&c->sc->eng, c->iobuf, BR_SSL_BUFSIZE_BIDI, 1);
    if (!br_ssl_client_reset(c->sc, host, 0)) {
        LOG_ERROR("TLS: client_reset failed for %s", host);
        conn_close(c);
        return NULL;
    }
    br_sslio_init(&c->ioc, &c->sc->eng, low_read_cb, c, low_write_cb, c);

    //
    // The handshake runs lazily on the first sslio_read/write; force it
    // here by writing zero bytes so callers see early failures.
    //

    if (br_sslio_flush(&c->ioc) < 0) {
        int err = br_ssl_engine_last_error(&c->sc->eng);
        LOG_ERROR("TLS handshake to %s failed: BearSSL err=%d", host, err);
        conn_close(c);
        return NULL;
    }
    return c;
}

ssize_t conn_read(conn_t *c, void *dst, size_t len, volatile int *cancel)
{
    if (!c) return -1;
    c->cancel = cancel;
    if (cancel && *cancel) return -2;

    if (c->is_tls) {
        int n = br_sslio_read(&c->ioc, dst, len);
        if (n < 0) {
            int err = br_ssl_engine_last_error(&c->sc->eng);
            if (err == BR_ERR_OK) return 0;          // clean close
            if (cancel && *cancel) return -2;
            return -1;
        }
        return n;
    }

    while (1) {
        if (cancel && *cancel) return -2;
        fd_set rfds;
        FD_ZERO(&rfds);
        FD_SET(c->fd, &rfds);
        struct timeval tv = { 1, 0 };
        int s = select(c->fd + 1, &rfds, NULL, NULL, &tv);
        if (s < 0) { if (errno == EINTR) continue; return -1; }
        if (s == 0) continue;
        ssize_t r = recv(c->fd, (char *)dst, len, 0);
        if (r < 0) { if (errno == EINTR) continue; return -1; }
        return r;
    }
}

int conn_write_all(conn_t *c, const void *src, size_t len)
{
    if (!c) return -1;
    if (c->is_tls) {
        c->cancel = NULL;
        if (br_sslio_write_all(&c->ioc, src, len) < 0) return -1;
        if (br_sslio_flush(&c->ioc) < 0) return -1;
        return 0;
    }

    const char *p = src;
    while (len) {
        ssize_t w = send(c->fd, p, len, MSG_NOSIGNAL);
        if (w < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        p += w;
        len -= (size_t)w;
    }
    return 0;
}

void conn_close(conn_t *c)
{
    if (!c) return;
    if (c->is_tls) {
        if (c->sc) {
            //
            // Best-effort TLS close-notify; don't care if it fails.
            //
            br_sslio_close(&c->ioc);
        }
        free(c->iobuf);
        free(c->sc);
        free(c->xc);
    }
    if (c->fd >= 0) close_sock(c->fd);
    free(c);
}
