#include "tiny.h"

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <pthread.h>

#if !defined(_WIN32)
  #include <fcntl.h>
  #include <unistd.h>
#else
  #include <bcrypt.h>
  #include <io.h>
#endif

static pthread_mutex_t log_mu = PTHREAD_MUTEX_INITIALIZER;

void log_msg(const char *level, const char *fmt, ...)
{
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    struct tm tm = {0};
    tiny_gmtime_r(&ts.tv_sec, &tm);

    pthread_mutex_lock(&log_mu);
    fprintf(stderr, "%04d-%02d-%02dT%02d:%02d:%02d.%03ldZ %-5s ",
            tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday,
            tm.tm_hour, tm.tm_min, tm.tm_sec, ts.tv_nsec / 1000000,
            level);
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fputc('\n', stderr);
    pthread_mutex_unlock(&log_mu);
}

void rand_bytes(unsigned char *out, size_t n)
{
#if defined(_WIN32)
    /* BCryptGenRandom with system-preferred RNG. */
    if (BCryptGenRandom(NULL, out, (ULONG)n, BCRYPT_USE_SYSTEM_PREFERRED_RNG) == 0)
        return;
    for (size_t i = 0; i < n; i++) out[i] = (unsigned char)rand();
#else
    int fd = open("/dev/urandom", O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        for (size_t i = 0; i < n; i++) out[i] = (unsigned char)rand();
        return;
    }
    size_t got = 0;
    while (got < n) {
        ssize_t r = read(fd, out + got, n - got);
        if (r <= 0) {
            if (errno == EINTR) continue;
            for (size_t i = got; i < n; i++) out[i] = (unsigned char)rand();
            break;
        }
        got += (size_t)r;
    }
    close(fd);
#endif
}

void uuid_v4(char out[37])
{
    unsigned char b[16];
    rand_bytes(b, 16);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    static const char hx[] = "0123456789abcdef";
    char *p = out;
    for (int i = 0; i < 16; i++) {
        if (i == 4 || i == 6 || i == 8 || i == 10) *p++ = '-';
        *p++ = hx[b[i] >> 4];
        *p++ = hx[b[i] & 0x0f];
    }
    *p = 0;
}

uint64_t monotonic_ms(void)
{
#if defined(_WIN32)
    return (uint64_t)GetTickCount64();
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000ULL + (uint64_t)ts.tv_nsec / 1000000ULL;
#endif
}

void buf_reserve(buf *b, size_t need)
{
    if (b->cap - b->len >= need) return;
    size_t cap = b->cap ? b->cap : 64;
    while (cap - b->len < need) cap *= 2;
    char *p = realloc(b->data, cap);
    if (!p) {
        LOG_ERROR("buf_reserve: out of memory (%zu)", cap);
        abort();
    }
    b->data = p;
    b->cap = cap;
}

void buf_putc(buf *b, char c)         { buf_reserve(b, 1); b->data[b->len++] = c; }
void buf_put (buf *b, const void *p, size_t n)
{
    buf_reserve(b, n);
    memcpy(b->data + b->len, p, n);
    b->len += n;
}
void buf_puts(buf *b, const char *s)  { buf_put(b, s, strlen(s)); }

void buf_putf(buf *b, const char *fmt, ...)
{
    va_list ap, ap2;
    va_start(ap, fmt);
    va_copy(ap2, ap);
    int n = vsnprintf(NULL, 0, fmt, ap);
    va_end(ap);
    if (n < 0) { va_end(ap2); return; }
    buf_reserve(b, (size_t)n + 1);
    vsnprintf(b->data + b->len, (size_t)n + 1, fmt, ap2);
    va_end(ap2);
    b->len += (size_t)n;
}

void buf_free(buf *b)
{
    free(b->data);
    b->data = NULL;
    b->len = b->cap = 0;
}

int is_privileged(void)
{
#if defined(_WIN32)
    /* Best-effort admin check via process token. */
    HANDLE tok = NULL;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &tok)) return 0;
    TOKEN_ELEVATION te = {0};
    DWORD got = 0;
    int admin = 0;
    if (GetTokenInformation(tok, TokenElevation, &te, sizeof(te), &got))
        admin = te.TokenIsElevated ? 1 : 0;
    CloseHandle(tok);
    return admin;
#else
    return geteuid() == 0;
#endif
}

int net_startup(void)
{
#if defined(_WIN32)
    WSADATA wsa;
    int rc = WSAStartup(MAKEWORD(2, 2), &wsa);
    if (rc != 0) {
        LOG_ERROR("WSAStartup failed (%d)", rc);
        return -1;
    }
#endif
    return 0;
}

void net_cleanup(void)
{
#if defined(_WIN32)
    WSACleanup();
#endif
}
