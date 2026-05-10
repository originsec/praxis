# praxis_node_tiny_c — pure C minimal Praxis node.
#
# Runtime dependencies:
#   - Linux/macOS:  libc + libpthread only.
#   - Windows:      libc (msvcrt) + ws2_32 (Winsock) + bcrypt + pthread
#                   (winpthreads from mingw-w64 is statically linked when
#                   built with -static).
#
# All protocol code (AMQP 0-9-1, HTTP/1.1, JSON, ACP JSON-RPC) is
# hand-rolled and compiled into the binary.

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

SRCS = $(SRC_DIR)/util.c  \
       $(SRC_DIR)/json.c  \
       $(SRC_DIR)/http.c  \
       $(SRC_DIR)/amqp.c  \
       $(SRC_DIR)/praxis.c\
       $(SRC_DIR)/main.c

OBJS = $(SRCS:.c=.o)

.PHONY: all clean release windows

all: $(BIN)

$(BIN): $(OBJS)
	$(CC) $(LDFLAGS) -o $@ $(OBJS) $(LIBS)

%.o: %.c $(SRC_DIR)/tiny.h
	$(CC) $(CFLAGS) -I$(SRC_DIR) -c -o $@ $<

# Smaller release build, stripped. NDEBUG drops LOG_DEBUG entirely.
release: CFLAGS  = $(CFLAGS_REL)
release: LDFLAGS = $(STRIP_LDFLAGS)
release: clean $(BIN)

# Cross-compile for Windows via mingw-w64. Produces praxis_node_tiny_c.exe.
# Requires x86_64-w64-mingw32-gcc on PATH (apt install mingw-w64).
windows:
	$(MAKE) clean
	$(MAKE) release \
	    CC=x86_64-w64-mingw32-gcc \
	    BIN=praxis_node_tiny_c.exe \
	    LIBS="-lws2_32 -lbcrypt -lpthread" \
	    STRIP_LDFLAGS="-Wl,--gc-sections -Wl,-s -Wl,--build-id=none -static"

clean:
	rm -f $(SRC_DIR)/*.o praxis_node_tiny_c praxis_node_tiny_c.exe
