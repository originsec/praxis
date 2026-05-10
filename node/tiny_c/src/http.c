#include "tiny.h"

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if !defined(_WIN32)
  #include <sys/select.h>
#endif

int http_parse_url(const char *url, char **out_host, int *out_port, char **out_path)
{
    const char *p = url;
    int port = 80;
    if (strncmp(p, "http://", 7) == 0) {
        p += 7;
    } else if (strncmp(p, "https://", 8) == 0) {
        LOG_ERROR("https endpoints are not supported by node_tiny_c (TLS not bundled)");
        return -1;
    }

    const char *host_end = p;
    while (*host_end && *host_end != ':' && *host_end != '/') host_end++;
    size_t hlen = (size_t)(host_end - p);
    if (hlen == 0) return -1;

    char *host = malloc(hlen + 1);
    if (!host) return -1;
    memcpy(host, p, hlen);
    host[hlen] = 0;

    if (*host_end == ':') {
        port = atoi(host_end + 1);
        while (*host_end && *host_end != '/') host_end++;
    }

    const char *path = (*host_end == '/') ? host_end : "/";
    char *pdup = strdup(path);
    if (!pdup) { free(host); return -1; }

    *out_host = host;
    *out_port = port;
    *out_path = pdup;
    return 0;
}

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

static int write_all(int fd, const void *buf, size_t n)
{
    const char *p = buf;
    while (n) {
        ssize_t w = send(fd, p, n, MSG_NOSIGNAL);
        if (w < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        p += w;
        n -= (size_t)w;
    }
    return 0;
}

/* read up to want bytes, append to b. Honors cancel via *cancel.
 * Returns >0 bytes read, 0 EOF, -1 error, -2 cancel. */
static ssize_t read_some(int fd, buf *b, size_t want, volatile int *cancel)
{
    if (cancel && *cancel) return -2;
    char tmp[4096];
    if (want > sizeof(tmp)) want = sizeof(tmp);
    fd_set rfds;
    struct timeval tv;
    while (1) {
        if (cancel && *cancel) return -2;
        FD_ZERO(&rfds);
        FD_SET(fd, &rfds);
        tv.tv_sec = 1;
        tv.tv_usec = 0;
        int s = select(fd + 1, &rfds, NULL, NULL, &tv);
        if (s < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        if (s == 0) continue;
        ssize_t r = recv(fd, tmp, want, 0);
        if (r < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        if (r == 0) return 0;
        buf_put(b, tmp, (size_t)r);
        return r;
    }
}

/* skip past CRLFCRLF in b starting at *off. Returns 1 once found and
 * updates *off to point after it. 0 means more data needed. */
static int find_headers_end(buf *b, size_t *off)
{
    if (b->len < 4) return 0;
    for (size_t i = *off; i + 4 <= b->len; i++) {
        if (b->data[i] == '\r' && b->data[i + 1] == '\n' &&
            b->data[i + 2] == '\r' && b->data[i + 3] == '\n') {
            *off = i + 4;
            return 1;
        }
    }
    *off = b->len >= 3 ? b->len - 3 : 0;
    return 0;
}

/* Process one chunk of accumulated body data: extract complete "data:"
 * lines and dispatch via on_chunk. Lines are terminated by \n.  We
 * preserve any partial trailing line at the front of the buffer. */
static void emit_sse(buf *body, void (*on_chunk)(const char *, size_t, void *), void *ud)
{
    size_t start = 0;
    for (size_t i = 0; i < body->len; i++) {
        if (body->data[i] != '\n') continue;
        size_t end = i;
        if (end > start && body->data[end - 1] == '\r') end--;
        size_t llen = end - start;
        const char *line = body->data + start;

        /* "data:" prefix with optional space */
        if (llen >= 5 && memcmp(line, "data:", 5) == 0) {
            const char *p = line + 5;
            size_t plen = llen - 5;
            if (plen > 0 && *p == ' ') { p++; plen--; }
            if (plen > 0) on_chunk(p, plen, ud);
        }
        start = i + 1;
    }
    if (start) {
        memmove(body->data, body->data + start, body->len - start);
        body->len -= start;
    }
}

int http_post_sse(const char *host, int port, const char *path,
                  const char *const *headers,
                  const void *body, size_t body_len,
                  void (*on_chunk)(const char *data, size_t n, void *ud),
                  void *ud,
                  volatile int *cancel)
{
    int fd = tcp_connect(host, port);
    if (fd < 0) return -1;

    buf req = {0};
    buf_putf(&req, "POST %s HTTP/1.1\r\n", path);
    buf_putf(&req, "Host: %s:%d\r\n", host, port);
    buf_puts(&req, "Connection: close\r\n");
    buf_puts(&req, "Accept: text/event-stream\r\n");
    buf_putf(&req, "Content-Length: %zu\r\n", body_len);
    if (headers) {
        for (const char *const *h = headers; *h; h++) {
            buf_puts(&req, *h);
            buf_puts(&req, "\r\n");
        }
    }
    buf_puts(&req, "\r\n");
    buf_put(&req, body, body_len);

    int rc = write_all(fd, req.data, req.len);
    buf_free(&req);
    if (rc < 0) { close_sock(fd); return -1; }

    /* read until headers fully received */
    buf raw = {0};
    size_t scan_off = 0;
    while (!find_headers_end(&raw, &scan_off)) {
        ssize_t r = read_some(fd, &raw, 4096, cancel);
        if (r == 0) { close_sock(fd); buf_free(&raw); return -1; }
        if (r < 0)  { close_sock(fd); buf_free(&raw); return r == -2 ? -2 : -1; }
    }

    /* parse status line */
    int status = 0;
    {
        const char *eol = memchr(raw.data, '\n', raw.len);
        if (!eol) { close_sock(fd); buf_free(&raw); return -1; }
        const char *sp1 = memchr(raw.data, ' ', (size_t)(eol - raw.data));
        if (!sp1) { close_sock(fd); buf_free(&raw); return -1; }
        status = atoi(sp1 + 1);
    }

    if (status < 200 || status >= 300) {
        LOG_WARN("HTTP POST %s returned status %d", path, status);
        /* drain a bit to log body */
        buf body_tail = {0};
        buf_put(&body_tail, raw.data + scan_off, raw.len - scan_off);
        for (int i = 0; i < 4; i++) {
            ssize_t r = read_some(fd, &body_tail, 1024, cancel);
            if (r <= 0) break;
        }
        if (body_tail.len) {
            size_t show = body_tail.len < 240 ? body_tail.len : 240;
            LOG_WARN("body: %.*s", (int)show, body_tail.data);
        }
        buf_free(&body_tail);
        buf_free(&raw);
        close_sock(fd);
        return -1;
    }

    /* check for chunked transfer encoding */
    int chunked = 0;
    {
        char headers_low[8192];
        size_t hlen = scan_off < sizeof(headers_low) ? scan_off : sizeof(headers_low) - 1;
        for (size_t i = 0; i < hlen; i++) {
            char c = raw.data[i];
            headers_low[i] = (c >= 'A' && c <= 'Z') ? (char)(c | 0x20) : c;
        }
        headers_low[hlen] = 0;
        if (strstr(headers_low, "transfer-encoding: chunked")) chunked = 1;
    }

    /* peel off body bytes already in raw */
    buf body_buf = {0};
    if (raw.len > scan_off) buf_put(&body_buf, raw.data + scan_off, raw.len - scan_off);
    buf_free(&raw);

    int ret = 0;
    if (!chunked) {
        emit_sse(&body_buf, on_chunk, ud);
        while (1) {
            ssize_t r = read_some(fd, &body_buf, 4096, cancel);
            if (r == 0) break;
            if (r == -2) { ret = -2; break; }
            if (r < 0)   { ret = -1; break; }
            emit_sse(&body_buf, on_chunk, ud);
        }
    } else {
        /* dechunk in place into a separate decoded buffer */
        buf decoded = {0};
        size_t need = 0;       /* bytes left in current chunk */
        int    in_size = 1;    /* parsing chunk-size line */
        while (1) {
            if (in_size) {
                /* find a CRLF */
                char *crlf = NULL;
                for (size_t i = 0; i + 1 < body_buf.len; i++) {
                    if (body_buf.data[i] == '\r' && body_buf.data[i + 1] == '\n') {
                        crlf = body_buf.data + i;
                        break;
                    }
                }
                if (!crlf) {
                    ssize_t r = read_some(fd, &body_buf, 4096, cancel);
                    if (r == 0) { ret = -1; break; }
                    if (r == -2) { ret = -2; break; }
                    if (r < 0)   { ret = -1; break; }
                    continue;
                }
                size_t hexlen = (size_t)(crlf - body_buf.data);
                char tmp[32];
                if (hexlen >= sizeof(tmp)) { ret = -1; break; }
                memcpy(tmp, body_buf.data, hexlen);
                tmp[hexlen] = 0;
                /* trim chunk extensions after ;  */
                char *semi = strchr(tmp, ';');
                if (semi) *semi = 0;
                need = strtoul(tmp, NULL, 16);
                size_t consume = hexlen + 2;
                memmove(body_buf.data, body_buf.data + consume, body_buf.len - consume);
                body_buf.len -= consume;
                in_size = 0;
                if (need == 0) {
                    /* trailers; we don't care */
                    break;
                }
            }
            if (body_buf.len < need + 2) {
                ssize_t r = read_some(fd, &body_buf, need + 2 - body_buf.len, cancel);
                if (r == 0)  { ret = -1; break; }
                if (r == -2) { ret = -2; break; }
                if (r < 0)   { ret = -1; break; }
                continue;
            }
            buf_put(&decoded, body_buf.data, need);
            memmove(body_buf.data, body_buf.data + need + 2, body_buf.len - need - 2);
            body_buf.len -= need + 2;
            in_size = 1;
            emit_sse(&decoded, on_chunk, ud);
        }
        buf_free(&decoded);
    }

    buf_free(&body_buf);
    close_sock(fd);
    return ret;
}
