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
// Custom X.509 "validator" that accepts every certificate without any
// chain or hostname validation, but still extracts the leaf cert's
// public key so the TLS engine can complete the handshake.
//
// This is INTENTIONAL for praxis_node_tiny_c: skipping cert validation
// drops the system trust-anchor table (~24 KB compiled in) and the
// X.509 minimal verifier's runtime state. The trade-off is that the
// agent endpoint is no longer authenticated — anyone able to intercept
// traffic between the node and the AI endpoint can read or modify it.
// Acceptable here because tiny_c is meant for trusted network paths
// (local proxies, in-cluster traffic, etc.); use the full Rust node
// if you need cert verification.
//

typedef struct {
    const br_x509_class      *vtable;
    br_x509_decoder_context   dec;
    int                       is_leaf;
} noverify_ctx;

static void nv_start_chain(const br_x509_class **ctx, const char *server_name)
{
    (void)server_name;
    noverify_ctx *c = (noverify_ctx *)ctx;
    c->is_leaf = 1;
    br_x509_decoder_init(&c->dec, NULL, NULL);
}

static void nv_start_cert(const br_x509_class **ctx, uint32_t length)
{
    (void)ctx; (void)length;
}

static void nv_append(const br_x509_class **ctx,
                      const unsigned char *buf, size_t len)
{
    noverify_ctx *c = (noverify_ctx *)ctx;
    if (c->is_leaf) br_x509_decoder_push(&c->dec, buf, len);
}

static void nv_end_cert(const br_x509_class **ctx)
{
    noverify_ctx *c = (noverify_ctx *)ctx;
    //
    // Only the first cert in the chain is the server's leaf — that's
    // the one whose public key the TLS engine will use. Stop feeding
    // bytes into the decoder for subsequent CA certs.
    //
    c->is_leaf = 0;
}

static unsigned nv_end_chain(const br_x509_class **ctx)
{
    (void)ctx;
    return 0;  /* always success */
}

static const br_x509_pkey *nv_get_pkey(const br_x509_class *const *ctx,
                                       unsigned *usages)
{
    const noverify_ctx *c = (const noverify_ctx *)ctx;
    if (usages) *usages = BR_KEYTYPE_KEYX | BR_KEYTYPE_SIGN;
    return br_x509_decoder_get_pkey((br_x509_decoder_context *)&c->dec);
}

static const br_x509_class noverify_vtable = {
    sizeof(noverify_ctx),
    nv_start_chain,
    nv_start_cert,
    nv_append,
    nv_end_cert,
    nv_end_chain,
    nv_get_pkey,
};

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
    noverify_ctx            *nv;
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
    setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, (const char *)&one, sizeof(one));
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

    //
    // One-time warning so operators are aware the TLS layer is doing
    // an unauthenticated handshake.
    //
    static int warned = 0;
    if (!warned) {
        warned = 1;
        LOG_WARN("TLS: cert verification is DISABLED (tiny_c build); "
                 "use the full Rust node for verified endpoints");
    }

    c->sc     = calloc(1, sizeof(*c->sc));
    c->nv     = calloc(1, sizeof(*c->nv));
    c->iobuf  = malloc(BR_SSL_BUFSIZE_BIDI);
    if (!c->sc || !c->nv || !c->iobuf) {
        conn_close(c);
        return NULL;
    }

    //
    // br_ssl_client_init_full configures all the cipher suites and
    // hash functions; we throw away its X509 minimal validator and
    // wire in the no-verify one above. We pass a tiny stack-local
    // X509 minimal context only because init_full requires one — its
    // state is unused after the override.
    //

    br_x509_minimal_context dummy_xc;
    br_ssl_client_init_full(c->sc, &dummy_xc, NULL, 0);

    c->nv->vtable = &noverify_vtable;
    br_ssl_engine_set_x509(&c->sc->eng, &c->nv->vtable);

    br_ssl_engine_set_buffer(&c->sc->eng, c->iobuf, BR_SSL_BUFSIZE_BIDI, 1);
    if (!br_ssl_client_reset(c->sc, host, 0)) {
        LOG_ERROR("TLS: client_reset failed for %s", host);
        conn_close(c);
        return NULL;
    }
    br_sslio_init(&c->ioc, &c->sc->eng, low_read_cb, c, low_write_cb, c);

    //
    // Force the handshake here so callers see early failures.
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
        free(c->nv);
    }
    if (c->fd >= 0) close_sock(c->fd);
    free(c);
}
