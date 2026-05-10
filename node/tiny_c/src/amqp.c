#include "tiny.h"

#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if !defined(_WIN32)
  #include <sys/select.h>
#endif

/* AMQP 0-9-1 frame types */
#define FT_METHOD     1
#define FT_HEADER     2
#define FT_BODY       3
#define FT_HEARTBEAT  8
#define FRAME_END  0xCE

#define CLASS_CONNECTION  10
#define CLASS_CHANNEL     20
#define CLASS_EXCHANGE    40
#define CLASS_QUEUE       50
#define CLASS_BASIC       60

struct amqp {
    int             fd;
    pthread_mutex_t wmu;
    volatile int    shutdown;          /* set by amqp_request_shutdown */

    /* delivery accumulation state for the read loop */
    char     cur_consumer_tag[256];
    uint64_t cur_delivery_tag;
    buf      cur_body;
    uint64_t cur_body_size;
    int      have_method;               /* 1 = method seen, awaiting header */
    int      have_header;               /* 1 = header seen, awaiting body */
};

/* ------------ byte helpers ------------------------------------- */

static void put_u8 (buf *b, uint8_t  v) { buf_putc(b, (char)v); }
static void put_u16(buf *b, uint16_t v) { uint8_t t[2]={v>>8,v}; buf_put(b,t,2); }
static void put_u32(buf *b, uint32_t v) {
    uint8_t t[4]={v>>24,v>>16,v>>8,v};
    buf_put(b,t,4);
}
static void put_u64(buf *b, uint64_t v) {
    uint8_t t[8]={v>>56,v>>48,v>>40,v>>32,v>>24,v>>16,v>>8,v};
    buf_put(b,t,8);
}
static void put_shortstr(buf *b, const char *s)
{
    size_t n = strlen(s);
    if (n > 255) n = 255;
    put_u8(b, (uint8_t)n);
    buf_put(b, s, n);
}
static void put_longstr(buf *b, const void *p, size_t n)
{
    put_u32(b, (uint32_t)n);
    buf_put(b, p, n);
}
static void put_empty_table(buf *b) { put_u32(b, 0); }

static uint16_t rd_u16(const uint8_t *p) { return (uint16_t)((p[0]<<8) | p[1]); }
static uint32_t rd_u32(const uint8_t *p) {
    return ((uint32_t)p[0]<<24)|((uint32_t)p[1]<<16)|((uint32_t)p[2]<<8)|p[3];
}
static uint64_t rd_u64(const uint8_t *p) {
    return ((uint64_t)rd_u32(p) << 32) | rd_u32(p+4);
}

/* ------------ socket I/O --------------------------------------- */

static int sock_write_all(int fd, const void *buf_, size_t n)
{
    const char *p = buf_;
    while (n) {
        ssize_t w = send(fd, p, n, MSG_NOSIGNAL);
        if (w < 0) { if (errno == EINTR) continue; return -1; }
        p += w; n -= (size_t)w;
    }
    return 0;
}

/* Read exactly n bytes. Honors c->shutdown via a 200ms periodic select
 * timeout when the caller wanted infinite blocking. Returns 0 on success,
 * -1 on error, -2 on shutdown, -3 on timeout (only when timeout_ms >= 0
 * and no data arrived). */
static int sock_read_exact(struct amqp *c, void *buf_, size_t n, int timeout_ms)
{
    char *p = buf_;
    int caller_timeout = timeout_ms;
    while (n) {
        if (c->shutdown) return -2;
        fd_set rfds;
        FD_ZERO(&rfds);
        FD_SET(c->fd, &rfds);
        struct timeval tv;
        int slot_ms = timeout_ms >= 0 ? timeout_ms : 200;
        tv.tv_sec  = slot_ms / 1000;
        tv.tv_usec = (slot_ms % 1000) * 1000;
        int s = select((int)(c->fd + 1), &rfds, NULL, NULL, &tv);
        if (s < 0) {
#if !defined(_WIN32)
            if (errno == EINTR) continue;
#endif
            return -1;
        }
        if (s == 0) {
            if (caller_timeout >= 0) return -3;
            continue; /* periodic wakeup; recheck shutdown */
        }
        ssize_t r = recv(c->fd, p, (int)n, 0);
        if (r < 0) {
#if !defined(_WIN32)
            if (errno == EINTR) continue;
#endif
            return -1;
        }
        if (r == 0) return -1;
        p += r;
        n -= (size_t)r;
        caller_timeout = -1;
        timeout_ms = -1; /* once we have partial bytes, finish without timing out */
    }
    return 0;
}

/* ------------ frame I/O ---------------------------------------- */

/* send a method frame with already-encoded args buffer. Holds write mutex. */
static int send_method_locked(struct amqp *c, uint16_t channel,
                              uint16_t cls, uint16_t method,
                              const void *args, size_t args_len)
{
    uint8_t hdr[7];
    hdr[0] = FT_METHOD;
    hdr[1] = channel >> 8; hdr[2] = channel;
    uint32_t plen = 4 + (uint32_t)args_len;
    hdr[3] = plen >> 24; hdr[4] = plen >> 16; hdr[5] = plen >> 8; hdr[6] = plen;
    if (sock_write_all(c->fd, hdr, 7) < 0) return -1;
    uint8_t mh[4];
    mh[0] = cls >> 8; mh[1] = cls; mh[2] = method >> 8; mh[3] = method;
    if (sock_write_all(c->fd, mh, 4) < 0) return -1;
    if (args_len && sock_write_all(c->fd, args, args_len) < 0) return -1;
    uint8_t end = FRAME_END;
    return sock_write_all(c->fd, &end, 1);
}

static int send_method(struct amqp *c, uint16_t channel,
                       uint16_t cls, uint16_t method,
                       const void *args, size_t args_len)
{
    pthread_mutex_lock(&c->wmu);
    int rc = send_method_locked(c, channel, cls, method, args, args_len);
    pthread_mutex_unlock(&c->wmu);
    return rc;
}

/* read a frame; allocates payload (caller frees). Returns 0 on success. */
static int read_frame(struct amqp *c, uint8_t *type_out, uint16_t *chan_out,
                      uint8_t **payload_out, uint32_t *plen_out, int timeout_ms)
{
    uint8_t hdr[7];
    int rc = sock_read_exact(c, hdr, 7, timeout_ms);
    if (rc < 0) return rc;
    *type_out = hdr[0];
    *chan_out = rd_u16(hdr + 1);
    uint32_t plen = rd_u32(hdr + 3);
    *plen_out = plen;
    uint8_t *p = malloc(plen + 1);
    if (!p) return -1;
    rc = sock_read_exact(c, p, plen, -1);
    if (rc < 0) { free(p); return rc; }
    uint8_t end;
    rc = sock_read_exact(c, &end, 1, -1);
    if (rc < 0) { free(p); return rc; }
    if (end != FRAME_END) { free(p); return -1; }
    *payload_out = p;
    return 0;
}

/* read frames until we get a method frame on channel `chan` matching
 * (cls, method). Discards heartbeats. Returns 0 + payload pointer
 * advanced past class/method ids in *args, *args_len. */
static int read_method(struct amqp *c, uint16_t chan, uint16_t cls, uint16_t method,
                       uint8_t **args_out, uint32_t *args_len_out, uint8_t **alloc_out)
{
    while (1) {
        uint8_t  type;
        uint16_t fchan;
        uint8_t *payload;
        uint32_t plen;
        int rc = read_frame(c, &type, &fchan, &payload, &plen, -1);
        if (rc < 0) return rc;
        if (type == FT_HEARTBEAT) { free(payload); continue; }
        if (type != FT_METHOD || plen < 4) { free(payload); return -1; }
        uint16_t fcls = rd_u16(payload);
        uint16_t fmth = rd_u16(payload + 2);
        if (fchan != chan || fcls != cls || fmth != method) {
            /* try to gracefully report channel/conn close */
            if (fcls == CLASS_CHANNEL && fmth == 40) {
                LOG_ERROR("AMQP channel.close received");
            }
            if (fcls == CLASS_CONNECTION && fmth == 50) {
                LOG_ERROR("AMQP connection.close received");
            }
            free(payload);
            return -1;
        }
        *args_out = payload + 4;
        *args_len_out = plen - 4;
        *alloc_out = payload;
        return 0;
    }
}

/* ------------ connect handshake -------------------------------- */

static int do_handshake(struct amqp *c, const char *user, const char *pass)
{
    /* protocol header */
    static const uint8_t proto[8] = {'A','M','Q','P',0,0,9,1};
    if (sock_write_all(c->fd, proto, 8) < 0) return -1;

    uint8_t *args, *alloc;
    uint32_t alen;

    /* connection.start */
    if (read_method(c, 0, CLASS_CONNECTION, 10, &args, &alen, &alloc) < 0) {
        LOG_ERROR("connection.start receive failed");
        return -1;
    }
    free(alloc);

    /* connection.start-ok: client-properties (table), mechanism (sstr),
     * response (lstr), locale (sstr) */
    {
        buf a = {0};
        /* empty client properties table */
        put_empty_table(&a);
        put_shortstr(&a, "PLAIN");
        size_t ulen = strlen(user), plen = strlen(pass);
        size_t resp_len = 1 + ulen + 1 + plen;
        char *resp = malloc(resp_len);
        if (!resp) { buf_free(&a); return -1; }
        resp[0] = 0;
        memcpy(resp + 1, user, ulen);
        resp[1 + ulen] = 0;
        memcpy(resp + 2 + ulen, pass, plen);
        put_longstr(&a, resp, resp_len);
        free(resp);
        put_shortstr(&a, "en_US");
        int rc = send_method(c, 0, CLASS_CONNECTION, 11, a.data, a.len);
        buf_free(&a);
        if (rc < 0) return -1;
    }

    /* connection.tune */
    if (read_method(c, 0, CLASS_CONNECTION, 30, &args, &alen, &alloc) < 0) {
        LOG_ERROR("connection.tune receive failed");
        return -1;
    }
    /* args: channel-max (short), frame-max (long), heartbeat (short) */
    uint16_t chan_max = alen >= 2 ? rd_u16(args) : 0;
    uint32_t frame_max = alen >= 6 ? rd_u32(args + 2) : 131072;
    free(alloc);

    /* connection.tune-ok: same shape; disable heartbeat (0) */
    {
        buf a = {0};
        put_u16(&a, chan_max ? chan_max : 1);
        put_u32(&a, frame_max ? frame_max : 131072);
        put_u16(&a, 0);
        int rc = send_method(c, 0, CLASS_CONNECTION, 31, a.data, a.len);
        buf_free(&a);
        if (rc < 0) return -1;
    }

    /* connection.open: virtual-host (sstr), reserved-1 (sstr), reserved-2 (bit) */
    {
        buf a = {0};
        put_shortstr(&a, "/");
        put_shortstr(&a, "");
        put_u8(&a, 0);
        int rc = send_method(c, 0, CLASS_CONNECTION, 40, a.data, a.len);
        buf_free(&a);
        if (rc < 0) return -1;
    }
    if (read_method(c, 0, CLASS_CONNECTION, 41, &args, &alen, &alloc) < 0) {
        LOG_ERROR("connection.open-ok receive failed");
        return -1;
    }
    free(alloc);

    /* channel.open on channel 1 */
    {
        buf a = {0};
        put_shortstr(&a, "");
        int rc = send_method(c, 1, CLASS_CHANNEL, 10, a.data, a.len);
        buf_free(&a);
        if (rc < 0) return -1;
    }
    if (read_method(c, 1, CLASS_CHANNEL, 11, &args, &alen, &alloc) < 0) {
        LOG_ERROR("channel.open-ok receive failed");
        return -1;
    }
    free(alloc);

    LOG_INFO("AMQP connection established (frame-max=%u)", frame_max);
    return 0;
}

amqp *amqp_connect(const char *host, int port, const char *user, const char *pass)
{
    struct amqp *c = calloc(1, sizeof(*c));
    if (!c) return NULL;
    c->fd = -1;
    pthread_mutex_init(&c->wmu, NULL);

    char portstr[16];
    snprintf(portstr, sizeof(portstr), "%d", port);
    struct addrinfo hints = {0}, *res = NULL;
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    int rc = getaddrinfo(host, portstr, &hints, &res);
    if (rc != 0) {
        LOG_WARN("getaddrinfo %s: %s", host, gai_strerror(rc));
        amqp_close(c);
        return NULL;
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
    if (fd < 0) { LOG_WARN("AMQP TCP connect %s:%d failed", host, port); amqp_close(c); return NULL; }
    int one = 1;
    setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
    c->fd = fd;

    if (do_handshake(c, user, pass) < 0) {
        amqp_close(c);
        return NULL;
    }
    return c;
}

void amqp_close(amqp *c)
{
    if (!c) return;
    if (c->fd >= 0) {
#if defined(_WIN32)
        shutdown(c->fd, SD_BOTH);
#else
        shutdown(c->fd, SHUT_RDWR);
#endif
        close_sock(c->fd);
    }
    buf_free(&c->cur_body);
    pthread_mutex_destroy(&c->wmu);
    free(c);
}

void amqp_request_shutdown(amqp *c)
{
    if (!c) return;
    c->shutdown = 1;
    /* Tear down the socket so any in-flight recv unblocks immediately. */
    if (c->fd >= 0) {
#if defined(_WIN32)
        shutdown(c->fd, SD_BOTH);
#else
        shutdown(c->fd, SHUT_RDWR);
#endif
    }
}

/* ------------ exchange / queue --------------------------------- */

int amqp_queue_declare(amqp *c, const char *queue)
{
    buf a = {0};
    put_u16(&a, 0);                         /* reserved-1 */
    put_shortstr(&a, queue);
    put_u8(&a, 0);                          /* passive=0 durable=0 exclusive=0 auto-delete=0 no-wait=0 */
    put_empty_table(&a);
    int rc = send_method(c, 1, CLASS_QUEUE, 10, a.data, a.len);
    buf_free(&a);
    if (rc < 0) return -1;
    uint8_t *args, *alloc; uint32_t alen;
    if (read_method(c, 1, CLASS_QUEUE, 11, &args, &alen, &alloc) < 0) return -1;
    free(alloc);
    return 0;
}

int amqp_queue_declare_exclusive(amqp *c, char *out, size_t out_cap)
{
    buf a = {0};
    put_u16(&a, 0);
    put_shortstr(&a, "");                    /* server-named */
    /* bits: passive=0 durable=0 exclusive=1 auto-delete=1 no-wait=0 */
    put_u8(&a, 0x0C);
    put_empty_table(&a);
    int rc = send_method(c, 1, CLASS_QUEUE, 10, a.data, a.len);
    buf_free(&a);
    if (rc < 0) return -1;

    uint8_t *args, *alloc; uint32_t alen;
    if (read_method(c, 1, CLASS_QUEUE, 11, &args, &alen, &alloc) < 0) return -1;
    /* declare-ok: queue (sstr), message-count (long), consumer-count (long) */
    if (alen < 1) { free(alloc); return -1; }
    uint8_t qlen = args[0];
    if ((uint32_t)1 + qlen > alen) { free(alloc); return -1; }
    if ((size_t)qlen + 1 > out_cap) { free(alloc); return -1; }
    memcpy(out, args + 1, qlen);
    out[qlen] = 0;
    free(alloc);
    return 0;
}

int amqp_exchange_declare_fanout(amqp *c, const char *name)
{
    buf a = {0};
    put_u16(&a, 0);                          /* reserved-1 */
    put_shortstr(&a, name);
    put_shortstr(&a, "fanout");
    /* bits: passive=0 durable=0 auto-delete=0 internal=0 no-wait=0 */
    put_u8(&a, 0);
    put_empty_table(&a);
    int rc = send_method(c, 1, CLASS_EXCHANGE, 10, a.data, a.len);
    buf_free(&a);
    if (rc < 0) return -1;
    uint8_t *args, *alloc; uint32_t alen;
    if (read_method(c, 1, CLASS_EXCHANGE, 11, &args, &alen, &alloc) < 0) return -1;
    free(alloc);
    return 0;
}

int amqp_queue_bind(amqp *c, const char *queue, const char *exchange, const char *routing_key)
{
    buf a = {0};
    put_u16(&a, 0);
    put_shortstr(&a, queue);
    put_shortstr(&a, exchange);
    put_shortstr(&a, routing_key);
    put_u8(&a, 0);                           /* no-wait=0 */
    put_empty_table(&a);
    int rc = send_method(c, 1, CLASS_QUEUE, 20, a.data, a.len);
    buf_free(&a);
    if (rc < 0) return -1;
    uint8_t *args, *alloc; uint32_t alen;
    if (read_method(c, 1, CLASS_QUEUE, 21, &args, &alen, &alloc) < 0) return -1;
    free(alloc);
    return 0;
}

int amqp_basic_consume(amqp *c, const char *queue, const char *consumer_tag)
{
    buf a = {0};
    put_u16(&a, 0);
    put_shortstr(&a, queue);
    put_shortstr(&a, consumer_tag ? consumer_tag : "");
    /* bits: no-local=0 no-ack=0 exclusive=0 no-wait=0 */
    put_u8(&a, 0);
    put_empty_table(&a);
    int rc = send_method(c, 1, CLASS_BASIC, 20, a.data, a.len);
    buf_free(&a);
    if (rc < 0) return -1;
    uint8_t *args, *alloc; uint32_t alen;
    if (read_method(c, 1, CLASS_BASIC, 21, &args, &alen, &alloc) < 0) return -1;
    free(alloc);
    return 0;
}

/* basic.publish + content-header + body. Splits body into frames bounded
 * by the negotiated frame-max; we conservatively cap at 131072 - 8 = 131064
 * to leave room for the 7-byte frame header + 1-byte end marker. */
int amqp_basic_publish(amqp *c, const char *exchange, const char *routing_key,
                       const void *body, size_t body_len)
{
    pthread_mutex_lock(&c->wmu);
    int rc = -1;

    /* method frame */
    {
        buf a = {0};
        put_u16(&a, 0);                  /* reserved-1 */
        put_shortstr(&a, exchange ? exchange : "");
        put_shortstr(&a, routing_key);
        put_u8(&a, 0);                   /* mandatory=0 immediate=0 */
        if (send_method_locked(c, 1, CLASS_BASIC, 40, a.data, a.len) < 0) {
            buf_free(&a);
            goto out;
        }
        buf_free(&a);
    }

    /* header frame: class-id (2), weight (2)=0, body-size (8), property-flags (2)=0 */
    {
        uint8_t hdr[7];
        hdr[0] = FT_HEADER;
        hdr[1] = 0; hdr[2] = 1;          /* channel 1 */
        uint32_t plen = 14;
        hdr[3] = plen >> 24; hdr[4] = plen >> 16; hdr[5] = plen >> 8; hdr[6] = plen;
        if (sock_write_all(c->fd, hdr, 7) < 0) goto out;
        uint8_t body_hdr[14];
        body_hdr[0] = 0; body_hdr[1] = CLASS_BASIC;
        body_hdr[2] = 0; body_hdr[3] = 0;       /* weight */
        body_hdr[4] = (uint8_t)(body_len >> 56);
        body_hdr[5] = (uint8_t)(body_len >> 48);
        body_hdr[6] = (uint8_t)(body_len >> 40);
        body_hdr[7] = (uint8_t)(body_len >> 32);
        body_hdr[8] = (uint8_t)(body_len >> 24);
        body_hdr[9] = (uint8_t)(body_len >> 16);
        body_hdr[10] = (uint8_t)(body_len >> 8);
        body_hdr[11] = (uint8_t)body_len;
        body_hdr[12] = 0; body_hdr[13] = 0;     /* property-flags */
        if (sock_write_all(c->fd, body_hdr, 14) < 0) goto out;
        uint8_t end = FRAME_END;
        if (sock_write_all(c->fd, &end, 1) < 0) goto out;
    }

    /* body frames */
    const size_t MAX_BODY_PER_FRAME = 131064;
    const char *p = body;
    size_t left = body_len;
    while (left > 0) {
        size_t take = left < MAX_BODY_PER_FRAME ? left : MAX_BODY_PER_FRAME;
        uint8_t hdr[7];
        hdr[0] = FT_BODY;
        hdr[1] = 0; hdr[2] = 1;
        uint32_t plen = (uint32_t)take;
        hdr[3] = plen >> 24; hdr[4] = plen >> 16; hdr[5] = plen >> 8; hdr[6] = plen;
        if (sock_write_all(c->fd, hdr, 7) < 0) goto out;
        if (sock_write_all(c->fd, p, take) < 0) goto out;
        uint8_t end = FRAME_END;
        if (sock_write_all(c->fd, &end, 1) < 0) goto out;
        p += take;
        left -= take;
    }
    rc = 0;
out:
    pthread_mutex_unlock(&c->wmu);
    return rc;
}

/* ------------ delivery loop ------------------------------------ */

static int basic_ack_locked(struct amqp *c, uint64_t delivery_tag)
{
    buf a = {0};
    put_u64(&a, delivery_tag);
    put_u8(&a, 0);                           /* multiple=0 */
    int rc = send_method_locked(c, 1, CLASS_BASIC, 80, a.data, a.len);
    buf_free(&a);
    return rc;
}

/* parse basic.deliver args into our cur_* fields. Returns 0/-1. */
static int parse_deliver(struct amqp *c, const uint8_t *args, uint32_t alen)
{
    if (alen < 1) return -1;
    uint8_t tlen = args[0];
    if ((uint32_t)1 + tlen > alen) return -1;
    size_t copy = tlen < sizeof(c->cur_consumer_tag) - 1 ? tlen : sizeof(c->cur_consumer_tag) - 1;
    memcpy(c->cur_consumer_tag, args + 1, copy);
    c->cur_consumer_tag[copy] = 0;
    if ((uint32_t)1 + tlen + 8 > alen) return -1;
    c->cur_delivery_tag = rd_u64(args + 1 + tlen);
    return 0;
}

int amqp_next_delivery_timeout(amqp *c, int timeout_ms,
                               char **body, size_t *body_len,
                               char *consumer_tag_out, size_t tag_cap)
{
    /* Reset accumulated body. */
    c->cur_body.len = 0;
    c->have_method = 0;
    c->have_header = 0;
    c->cur_body_size = 0;

    int first_pass = 1;
    while (1) {
        uint8_t  type;
        uint16_t fchan;
        uint8_t *payload;
        uint32_t plen;
        int slot_timeout = (first_pass && !c->have_method) ? timeout_ms : -1;
        int rc = read_frame(c, &type, &fchan, &payload, &plen, slot_timeout);
        if (rc == -2) return 0;
        if (rc == -3) return -2;
        if (rc < 0) return -1;
        first_pass = 0;

        if (type == FT_HEARTBEAT) { free(payload); continue; }

        if (type == FT_METHOD) {
            if (plen < 4) { free(payload); return -1; }
            uint16_t cls = rd_u16(payload);
            uint16_t mth = rd_u16(payload + 2);
            if (cls == CLASS_BASIC && mth == 60) {
                if (parse_deliver(c, payload + 4, plen - 4) < 0) {
                    free(payload);
                    return -1;
                }
                c->have_method = 1;
                free(payload);
                continue;
            }
            if (cls == CLASS_CHANNEL && mth == 40) {
                LOG_ERROR("AMQP channel.close in delivery loop");
                free(payload);
                return -1;
            }
            if (cls == CLASS_CONNECTION && mth == 50) {
                LOG_ERROR("AMQP connection.close in delivery loop");
                free(payload);
                return -1;
            }
            /* ignore other methods (e.g. basic.cancel) */
            free(payload);
            continue;
        }

        if (type == FT_HEADER) {
            if (!c->have_method || plen < 14) { free(payload); return -1; }
            c->cur_body_size = rd_u64(payload + 4);
            c->have_header = 1;
            free(payload);
            if (c->cur_body_size == 0) goto done;
            continue;
        }

        if (type == FT_BODY) {
            if (!c->have_header) { free(payload); return -1; }
            buf_put(&c->cur_body, payload, plen);
            free(payload);
            if (c->cur_body.len >= c->cur_body_size) goto done;
            continue;
        }

        free(payload);
    }

done:
    /* ack */
    pthread_mutex_lock(&c->wmu);
    basic_ack_locked(c, c->cur_delivery_tag);
    pthread_mutex_unlock(&c->wmu);

    if (consumer_tag_out && tag_cap)
        snprintf(consumer_tag_out, tag_cap, "%s", c->cur_consumer_tag);

    if (c->cur_body.len < c->cur_body_size) {
        /* short body: fill rest with zeros to be safe; should not happen */
        buf_putc(&c->cur_body, 0);
    }
    *body = c->cur_body.data;
    *body_len = (size_t)c->cur_body_size;
    return 1;
}

int amqp_next_delivery(amqp *c, char **body, size_t *body_len,
                       char *consumer_tag_out, size_t tag_cap)
{
    return amqp_next_delivery_timeout(c, -1, body, body_len, consumer_tag_out, tag_cap);
}
