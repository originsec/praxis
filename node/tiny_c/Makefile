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

CC         ?= cc
HOSTCC     ?= cc
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
# upstream tarball is downloaded once on demand.
#
# We build BearSSL into two separate output dirs:
#
#   build-host/  — always built with HOSTCC. Provides the `brssl` tool
#                  used to generate src/trust_anchors.inc from the
#                  system CA bundle. Always native to the build host so
#                  it can run during the build.
#
#   build/       — built with CC (which may be a cross compiler). The
#                  static lib that gets linked into the final binary.
#                  When CC == HOSTCC this is identical to build-host/
#                  and we just symlink to avoid a redundant build.
#
BEARSSL_VERSION = 0.6
BEARSSL_DIR     = vendor/bearssl-$(BEARSSL_VERSION)
BEARSSL_TARBALL = vendor/bearssl-$(BEARSSL_VERSION).tar.gz
BEARSSL_URL     = https://www.bearssl.org/bearssl-$(BEARSSL_VERSION).tar.gz
BEARSSL_HOST_DIR = $(BEARSSL_DIR)/build-host
BEARSSL_TGT_DIR  = $(BEARSSL_DIR)/build
BEARSSL_LIB      = $(BEARSSL_TGT_DIR)/libbearssl.a
BRSSL_HOST       = $(BEARSSL_HOST_DIR)/brssl
TA_PEM          ?= $(firstword $(wildcard /etc/ssl/certs/ca-certificates.crt \
                                          /etc/ca-certificates/extracted/tls-ca-bundle.pem \
                                          /etc/pki/tls/certs/ca-bundle.crt))
TA_FILE          = $(SRC_DIR)/trust_anchors.inc

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

%.o: %.c $(SRC_DIR)/tiny.h $(BEARSSL_LIB) $(TA_FILE)
	$(CC) $(CFLAGS) -I$(SRC_DIR) -c -o $@ $<

$(BEARSSL_TARBALL):
	mkdir -p vendor
	curl -fsSL -o $@ $(BEARSSL_URL)

$(BEARSSL_DIR)/Makefile: $(BEARSSL_TARBALL)
	tar -xzf $(BEARSSL_TARBALL) -C vendor
	touch $@

# Host build: always native, used for the brssl tool.
$(BRSSL_HOST): $(BEARSSL_DIR)/Makefile
	$(MAKE) -C $(BEARSSL_DIR) BUILD=build-host CC=$(HOSTCC) tools

# Target build: uses the (possibly cross-compiling) CC.
$(BEARSSL_LIB): $(BEARSSL_DIR)/Makefile
	$(MAKE) -C $(BEARSSL_DIR) BUILD=build CC="$(CC)" lib

$(TA_FILE): $(BRSSL_HOST)
	@if [ -z "$(TA_PEM)" ]; then \
	  echo "ERROR: no system CA bundle found. Set TA_PEM=/path/to/cabundle.pem"; \
	  exit 1; \
	fi
	@echo "Generating trust anchors from $(TA_PEM)"
	$(BRSSL_HOST) ta -q $(TA_PEM) > $@.tmp
	mv $@.tmp $@

# Smaller release build, stripped. NDEBUG drops LOG_DEBUG entirely.
release: CFLAGS  = $(CFLAGS_REL) -I$(BEARSSL_DIR)/inc -DTINY_C_TLS
release: LDFLAGS = $(STRIP_LDFLAGS)
release: clean $(BIN)

# Cross-compile for Windows via mingw-w64. Produces praxis_node_tiny_c.exe.
# Requires x86_64-w64-mingw32-gcc on PATH (apt install mingw-w64).
#
# Trust anchors are generated using a host-native BearSSL build (the
# resulting cert table is OS-independent), then the library is rebuilt
# with the cross compiler before linking.
windows:
	$(MAKE) clean
	rm -rf $(BEARSSL_TGT_DIR)
	$(MAKE) release \
	    CC=x86_64-w64-mingw32-gcc \
	    HOSTCC=$(HOSTCC) \
	    BIN=praxis_node_tiny_c.exe \
	    LIBS="-lws2_32 -lbcrypt -lpthread" \
	    STRIP_LDFLAGS="-Wl,--gc-sections -Wl,-s -Wl,--build-id=none -static"

clean:
	rm -f $(SRC_DIR)/*.o praxis_node_tiny_c praxis_node_tiny_c.exe $(TA_FILE) $(TA_FILE).tmp

# Wipes the vendored BearSSL build too; next 'make' re-fetches and rebuilds.
distclean: clean
	rm -rf vendor
