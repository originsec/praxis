/*
 * praxis_node_tiny_c — minimal pure-C praxis node.
 *
 * Runtime dependencies: libc only on Linux/macOS. On Windows the
 * binary additionally links the system Winsock + bcrypt DLLs (which
 * play the same "always present" role as libc). All protocol code
 * (AMQP 0-9-1, HTTP/1.1, JSON, ACP JSON-RPC) is hand-rolled and
 * statically linked.
 *
 * Scope: register with the praxis service over RabbitMQ, host an ACP
 * server for the native Praxis agent, run shell commands as tool
 * calls, stream chat completions back to clients via session/update
 * notifications.
 *
 * Limitations:
 *   - OpenAI-compatible chat-completions API only; no Anthropic or
 *     Gemini provider plumbing.
 *
 * TLS: BearSSL (vendored under vendor/) is statically linked. Trust
 * anchors are generated at build time from the system CA bundle.
 */

#ifndef TINY_H
#define TINY_H

#include <stddef.h>
#include <stdint.h>
#include <stdarg.h>
#include <sys/types.h>

/* ============================================================== */
/* platform                                                         */
/* ============================================================== */

#if defined(_WIN32)
  #ifndef WIN32_LEAN_AND_MEAN
    #define WIN32_LEAN_AND_MEAN
  #endif
  #include <winsock2.h>
  #include <ws2tcpip.h>
  #include <windows.h>
  #include <time.h>
  typedef SSIZE_T ssize_t;
  #define close_sock closesocket
  #ifndef MSG_NOSIGNAL
    #define MSG_NOSIGNAL 0
  #endif
  #ifndef SOCK_CLOEXEC
    #define SOCK_CLOEXEC 0
  #endif

  //
  // MinGW exposes gmtime_s only when __STDC_WANT_LIB_EXT1__ is set,
  // which we'd rather not require. gmtime() returns a pointer into a
  // static buffer; copying the result immediately is good enough for
  // log timestamps.
  //

  static inline struct tm *tiny_gmtime_r(const time_t *t, struct tm *out) {
      struct tm *r = gmtime(t);
      if (!r) return NULL;
      *out = *r;
      return out;
  }
  static inline void sleep_ms(unsigned ms) { Sleep(ms); }
#else
  #include <sys/socket.h>
  #include <netinet/in.h>
  #include <netinet/tcp.h>
  #include <netdb.h>
  #include <unistd.h>
  #include <time.h>
  #define close_sock close
  /* macOS lacks MSG_NOSIGNAL; we ignore SIGPIPE globally. */
  #ifndef MSG_NOSIGNAL
    #define MSG_NOSIGNAL 0
  #endif
  /* macOS lacks SOCK_CLOEXEC; benign no-op. */
  #ifndef SOCK_CLOEXEC
    #define SOCK_CLOEXEC 0
  #endif
  static inline struct tm *tiny_gmtime_r(const time_t *t, struct tm *out) {
      return gmtime_r(t, out);
  }
  static inline void sleep_ms(unsigned ms) {
      struct timespec ts = { ms / 1000, (long)(ms % 1000) * 1000000L };
      nanosleep(&ts, NULL);
  }
#endif

/* Initialize platform networking. Call once at startup. Returns 0/-1. */
int net_startup(void);
void net_cleanup(void);

/* ============================================================== */
/* util.c — logging, time, random, dynamic buffers, base64         */
/* ============================================================== */

void log_msg(const char *level, const char *fmt, ...);
#define LOG_INFO(...)  log_msg("INFO",  __VA_ARGS__)
#define LOG_WARN(...)  log_msg("WARN",  __VA_ARGS__)
#define LOG_ERROR(...) log_msg("ERROR", __VA_ARGS__)

/*
 * LOG_DEBUG is compiled out entirely in release builds. The release
 * Makefile target defines NDEBUG; the debug target does not.
 */
#ifdef NDEBUG
#define LOG_DEBUG(...) ((void)0)
#else
#define LOG_DEBUG(...) log_msg("DEBUG", __VA_ARGS__)
#endif

void rand_bytes(unsigned char *out, size_t n);
void uuid_v4(char out[37]);

uint64_t monotonic_ms(void);

/* Growing byte buffer. Owns its memory; double-free-safe via len=0/cap=0. */
typedef struct buf {
    char  *data;
    size_t len;
    size_t cap;
} buf;

void buf_reserve(buf *b, size_t need);
void buf_putc(buf *b, char c);
void buf_put(buf *b, const void *p, size_t n);
void buf_puts(buf *b, const char *s);
void buf_putf(buf *b, const char *fmt, ...);
void buf_free(buf *b);

/* current process is privileged (uid 0)? */
int is_privileged(void);

/* ============================================================== */
/* json.c — parser + writer                                        */
/* ============================================================== */

typedef enum {
    JNULL, JBOOL, JNUM, JSTR, JARR, JOBJ
} json_type;

typedef struct json {
    json_type type;
    union {
        int     b;
        double  n;
        struct { char *s; size_t len; } str;
        struct { struct json **items; size_t count; } arr;
        struct {
            char         **keys;
            size_t        *key_lens;
            struct json  **vals;
            size_t         count;
        } obj;
    } u;
} json;

/* Parse src..src+n. Returns owned tree on success, NULL on parse error.
 * The returned value owns all sub-allocations and is freed via json_free. */
json *json_parse(const char *src, size_t n);
void  json_free(json *j);

/* Path lookup: dot-separated keys (foo.bar.baz). NULL if not found. */
json *json_get(json *j, const char *path);

/* Type helpers: return 0/empty on type mismatch. */
const char *json_str(json *j, size_t *len_out);
int  json_get_str(json *j, const char *path, const char **out, size_t *len_out);
int  json_get_bool(json *j, const char *path, int *out);
int  json_get_int (json *j, const char *path, long *out);

/* Writer helpers — appends to a buf. The string variants quote+escape. */
void jb_str(buf *b, const char *s, size_t n);   /* "...escaped..." */
void jb_strz(buf *b, const char *s);            /* same but null-term */

/* ============================================================== */
/* amqp.c — AMQP 0-9-1 client                                       */
/* ============================================================== */

typedef struct amqp amqp;

amqp *amqp_connect(const char *host, int port, const char *user, const char *pass);
void  amqp_close(amqp *c);

int amqp_queue_declare(amqp *c, const char *queue);
int amqp_exchange_declare_fanout(amqp *c, const char *name);
/* Declare exclusive auto-delete server-named queue, write the actual
 * name into out (caller-owned buffer of out_cap bytes). */
int amqp_queue_declare_exclusive(amqp *c, char *out, size_t out_cap);
int amqp_queue_bind(amqp *c, const char *queue, const char *exchange, const char *routing_key);

int amqp_basic_publish(amqp *c, const char *exchange, const char *routing_key,
                       const void *body, size_t body_len);
int amqp_basic_consume(amqp *c, const char *queue, const char *consumer_tag);

/* Read one delivered message. *body / *body_len point into a buffer owned
 * by the amqp; valid until the next amqp_* call. Returns 1 on delivery,
 * 0 on shutdown signal, -1 on error. consumer_tag_out (optional) gets
 * the matching consumer tag. */
int amqp_next_delivery(amqp *c, char **body, size_t *body_len,
                       char *consumer_tag_out, size_t tag_cap);

/* Try to read a delivery, with timeout in milliseconds. -2 on timeout,
 * 0 on shutdown, 1 on delivery, -1 on error. */
int amqp_next_delivery_timeout(amqp *c, int timeout_ms,
                               char **body, size_t *body_len,
                               char *consumer_tag_out, size_t tag_cap);

/* Tear-down request: cause amqp_next_delivery* to return 0 ASAP. */
void amqp_request_shutdown(amqp *c);

/* ============================================================== */
/* http.c — minimal HTTP/1.1 client + SSE                           */
/* ============================================================== */

/* Parse url into host/port/path/use_tls. Both http:// and https:// are
 * supported. Caller owns out_host/out_path (heap). Returns 0/-1. */
int  http_parse_url(const char *url, char **out_host, int *out_port,
                    char **out_path, int *out_use_tls);

/* Send POST and stream back SSE chunks via on_chunk. headers is a
 * NULL-terminated array of "Key: value" strings.  Each "data:" line
 * payload is delivered to on_chunk. cancel is checked between reads;
 * if non-NULL and *cancel != 0, returns -2.  Returns 0 on clean stream
 * end, -1 on transport error, -2 on cancel. */
int http_post_sse(const char *host, int port, int use_tls, const char *path,
                  const char *const *headers,
                  const void *body, size_t body_len,
                  void (*on_chunk)(const char *data, size_t n, void *ud),
                  void *ud,
                  volatile int *cancel);

/* ============================================================== */
/* praxis.c — agent sessions, ACP dispatch, run_command             */
/* ============================================================== */

typedef struct praxis_cfg {
    char *provider;        /* unused, kept for parity */
    char *api_key;
    char *endpoint_url;
    char *model_name;
    char *system_prompt;   /* may be NULL */
    int   max_tool_iters;
    int   command_timeout_secs;
} praxis_cfg;

void praxis_cfg_free(praxis_cfg *c);

/* Apply a fresh praxis config (or NULL to disable). Takes ownership.
 * Subsequent session/new on the praxis connector uses this config. */
void praxis_set_config(praxis_cfg *cfg);
int  praxis_enabled(void);

/* Handle one inbound ACP JSON-RPC frame. Outbound frames are pushed via
 * acp_send_*(). Frame is parsed, dispatched, and freed.  Spawns a
 * background thread for session/prompt so the AMQP loop never blocks. */
void acp_handle_frame(const char *client_id, const char *rpc, size_t rpc_len);

/* Wire-out helpers used by acp dispatch. Defined in main.c so the AMQP
 * channel lives there. */
void acp_send_response(const char *client_id, const char *id_raw,
                       const char *result_raw);
void acp_send_error(const char *client_id, const char *id_raw,
                    int code, const char *msg);
void acp_send_session_notification(const char *client_id, const char *session_id,
                                   const char *update_raw);

/* main.c sets these so praxis.c can publish info updates. */
extern char tiny_node_id[64];

/* Build and send a NodeInformationUpdate. */
void send_node_information_update(void);

/* Wait for all in-flight session/prompt threads. */
void praxis_join_workers(void);

#endif /* TINY_H */
