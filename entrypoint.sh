#!/bin/bash
set -e

echo "Starting Praxis..."
echo "  RabbitMQ: $PRAXIS_RABBITMQ_URL"

# Extract host:port from AMQP URL for connectivity check.
RABBITMQ_HOST=$(echo "$PRAXIS_RABBITMQ_URL" | sed -E 's|amqp://[^@]+@([^/:]+):([0-9]+).*|\1|')
RABBITMQ_PORT=$(echo "$PRAXIS_RABBITMQ_URL" | sed -E 's|amqp://[^@]+@([^/:]+):([0-9]+).*|\2|')

echo "Waiting for RabbitMQ at $RABBITMQ_HOST:$RABBITMQ_PORT..."
for i in $(seq 1 30); do
    if nc -z "$RABBITMQ_HOST" "$RABBITMQ_PORT" 2>/dev/null; then
        echo "RabbitMQ is reachable."
        break
    fi
    echo "  Attempt $i/30 - waiting..."
    sleep 2
done

/app/praxis_service &
SERVICE_PID=$!
sleep 2

if ! kill -0 $SERVICE_PID 2>/dev/null; then
    echo "Error: praxis_service failed to start"
    exit 1
fi

/app/praxis_web &
WEB_PID=$!

echo "Praxis running."
echo "  Web UI: http://localhost:8080"

wait $SERVICE_PID $WEB_PID
