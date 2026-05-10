#ifndef CONN_H
#define CONN_H

#include <stddef.h>
#include <sys/types.h>

//
// Transport abstraction over plain TCP and TLS (BearSSL).  http.c
// uses this so the same code path serves both http:// and https://.
//

typedef struct conn conn_t;

//
// Open a connection. If use_tls != 0 the call also performs the TLS
// handshake (with BearSSL's full X.509 minimal verifier against the
// trust anchors compiled into the binary). Returns NULL on failure.
//

conn_t *conn_open(const char *host, int port, int use_tls);

//
// Read up to len bytes into dst.  Returns >0 bytes read, 0 on EOF, -1
// on error, -2 on cancel.  cancel may be NULL.
//

ssize_t conn_read(conn_t *c, void *dst, size_t len, volatile int *cancel);

//
// Write all bytes. Returns 0 on success, -1 on error.
//

int conn_write_all(conn_t *c, const void *src, size_t len);

void conn_close(conn_t *c);

#endif
