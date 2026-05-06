#!/bin/bash
#
# Wait until the RabbitMQ host:port from PRAXIS_RABBITMQ_URL is reachable.
# Used by the praxis-wait-rabbitmq.service oneshot inside the docker image.
#

set -e

URL="${PRAXIS_RABBITMQ_URL:-amqp://praxis:praxis@localhost:5672}"
HOST=$(echo "$URL" | sed -E 's|amqps?://([^@]+@)?([^/:?]+).*|\2|')
PORT=$(echo "$URL" | sed -E 's|.*:([0-9]+)(/.*)?$|\1|')
HOST="${HOST:-localhost}"
PORT="${PORT:-5672}"

echo "Waiting for RabbitMQ at $HOST:$PORT..."
for i in $(seq 1 60); do
    if (exec 3<>"/dev/tcp/$HOST/$PORT") 2>/dev/null; then
        exec 3<&-; exec 3>&-
        echo "RabbitMQ is reachable."
        exit 0
    fi
    sleep 2
done

echo "RabbitMQ not reachable after 120s, continuing anyway." >&2
exit 0
