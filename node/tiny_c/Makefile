# praxis_node_tiny_c — pure C minimal Praxis node.
#
# Runtime dependencies:
#   - Linux/macOS:  libc + libpthread only.
#   - Windows:      libc (msvcrt) + ws2_32 (Winsock) + bcrypt + pthread
#                   (winpthreads from mingw-w64 is statically linked when
#                   built with -static).
#
# All protocol code (AMQP 0-9-1, HTTP/1.1, JSON, ACP JSON-RPC) is
# hand-rolled and compiled into the binary. TLS uses BearSSL (smallest
# production TLS), built from a vendored source tree under vendor/.
# Cert verification is intentionally disabled — see src/conn.c.

CC         ?= cc
SRC_DIR     = src
BIN         = praxis_node_tiny_c

UNAME_S    := $(shell uname -s 2>/dev/null || echo unknown)

COMMON_CFLAGS = -Wall -Wextra -Wno-unused-parameter -std=c11 \
                -D_GNU_SOURCE -fno-strict-aliasing
CFLAGS_DEBUG  = -O2 -g $(COMMON_CFLAGS)
CFLAGS_REL    = -Os -DNDEBUG -ffunction-sections -fdata-sections \
                -fvisibility=hidden -fno-asynchronous-unwind-tables \
                -fno-unwind-tables $(COMMON_CFLAGS)

# Platform link flags
ifeq ($(UNAME_S),Darwin)
  LIBS         = -lpthread
  STRIP_LDFLAGS = -Wl,-dead_strip
else ifeq ($(findstring MINGW,$(UNAME_S)),MINGW)
  LIBS         = -lws2_32 -lbcrypt -lpthread
  STRIP_LDFLAGS = -Wl,--gc-sections -Wl,-s -Wl,--build-id=none -static
  BIN         := $(BIN).exe
else
  LIBS         = -lpthread
  STRIP_LDFLAGS = -Wl,--gc-sections -Wl,-s -Wl,--build-id=none
endif

CFLAGS  ?= $(CFLAGS_DEBUG)
LDFLAGS ?=

#
# BearSSL (TLS). Vendored under vendor/bearssl-$(BEARSSL_VERSION)/. The
# upstream tarball is downloaded once on demand. We only need the
# static library — the brssl tool is not required since cert
# verification is disabled (see src/conn.c).
#
BEARSSL_VERSION = 0.6
BEARSSL_DIR     = vendor/bearssl-$(BEARSSL_VERSION)
BEARSSL_TARBALL = vendor/bearssl-$(BEARSSL_VERSION).tar.gz
BEARSSL_URL     = https://www.bearssl.org/bearssl-$(BEARSSL_VERSION).tar.gz
BEARSSL_LIB     = $(BEARSSL_DIR)/build/libbearssl.a

CFLAGS += -I$(BEARSSL_DIR)/inc -DTINY_C_TLS

SRCS = $(SRC_DIR)/util.c  \
       $(SRC_DIR)/json.c  \
       $(SRC_DIR)/conn.c  \
       $(SRC_DIR)/http.c  \
       $(SRC_DIR)/amqp.c  \
       $(SRC_DIR)/praxis.c\
       $(SRC_DIR)/main.c

OBJS = $(SRCS:.c=.o)

.PHONY: all clean release windows distclean

all: $(BIN)

$(BIN): $(OBJS) $(BEARSSL_LIB)
	$(CC) $(LDFLAGS) -o $@ $(OBJS) $(BEARSSL_LIB) $(LIBS)

%.o: %.c $(SRC_DIR)/tiny.h $(BEARSSL_LIB)
	$(CC) $(CFLAGS) -I$(SRC_DIR) -c -o $@ $<

$(BEARSSL_TARBALL):
	mkdir -p vendor
	curl -fsSL -o $@ $(BEARSSL_URL)

$(BEARSSL_DIR)/Makefile: $(BEARSSL_TARBALL)
	tar -xzf $(BEARSSL_TARBALL) -C vendor
	touch $@

#
# BearSSL's vendored Unix.mk sets CFLAGS = "-W -Wall -Os -fPIC". Override
# it to add -Wno-unknown-pragmas so MinGW does not complain about the
# MSVC-only `#pragma comment(lib, ...)` in sysrng.c.
#
$(BEARSSL_LIB): $(BEARSSL_DIR)/Makefile
	$(MAKE) -C $(BEARSSL_DIR) BUILD=build CC="$(CC)" \
	    CFLAGS="-W -Wall -Wno-unknown-pragmas -Os -fPIC" lib

# Smaller release build, stripped. NDEBUG drops LOG_DEBUG entirely.
release: CFLAGS  = $(CFLAGS_REL) -I$(BEARSSL_DIR)/inc -DTINY_C_TLS
release: LDFLAGS = $(STRIP_LDFLAGS)
release: clean $(BIN)

# Cross-compile for Windows via mingw-w64. Produces praxis_node_tiny_c.exe.
# Requires x86_64-w64-mingw32-gcc on PATH (apt install mingw-w64).
windows:
	$(MAKE) clean
	rm -rf $(BEARSSL_DIR)/build
	$(MAKE) release \
	    CC=x86_64-w64-mingw32-gcc \
	    BIN=praxis_node_tiny_c.exe \
	    LIBS="-lws2_32 -lbcrypt -lpthread" \
	    STRIP_LDFLAGS="-Wl,--gc-sections -Wl,-s -Wl,--build-id=none -static"

clean:
	rm -f $(SRC_DIR)/*.o praxis_node_tiny_c praxis_node_tiny_c.exe

# Wipes the vendored BearSSL build too; next 'make' re-fetches and rebuilds.
distclean: clean
	rm -rf vendor
