# praxis_node_tiny_c — pure C minimal Praxis node.
#
# Runtime dependencies: libc only (and libpthread, which is part of glibc).
# Everything else is hand-rolled and statically linked at build time.

CC      ?= cc
CFLAGS  ?= -O2 -g -Wall -Wextra -Wno-unused-parameter -std=c11 \
           -D_GNU_SOURCE -fno-strict-aliasing
LDFLAGS ?=
LIBS     = -lpthread

SRC_DIR  = src
BIN      = praxis_node_tiny_c

SRCS = $(SRC_DIR)/util.c  \
       $(SRC_DIR)/json.c  \
       $(SRC_DIR)/http.c  \
       $(SRC_DIR)/amqp.c  \
       $(SRC_DIR)/praxis.c\
       $(SRC_DIR)/main.c

OBJS = $(SRCS:.c=.o)

.PHONY: all clean strip release

all: $(BIN)

$(BIN): $(OBJS)
	$(CC) $(LDFLAGS) -o $@ $(OBJS) $(LIBS)

%.o: %.c $(SRC_DIR)/tiny.h
	$(CC) $(CFLAGS) -I$(SRC_DIR) -c -o $@ $<

# Smaller release build, stripped. NDEBUG removes LOG_DEBUG and any
# debug-only code paths.
release: CFLAGS = -Os -Wall -Wextra -Wno-unused-parameter -std=c11 \
                  -D_GNU_SOURCE -DNDEBUG \
                  -fno-strict-aliasing -ffunction-sections -fdata-sections \
                  -fvisibility=hidden -fno-asynchronous-unwind-tables \
                  -fno-unwind-tables
release: LDFLAGS = -Wl,--gc-sections -Wl,-s -Wl,--build-id=none
release: clean $(BIN)

clean:
	rm -f $(OBJS) $(BIN)
