#!/bin/bash

TIMEOUT=60  # Timeout Time (seconds)
COUNT=0

while true; do
    if [ -e /var/run/cryptpilot/.network-online ]; then
        echo "Network is online."
        break
    fi
    sleep 1
    COUNT=$((COUNT + 1))

    if [ $COUNT -ge $TIMEOUT ]; then
        echo "Timeout after $TIMEOUT seconds waiting for network."
        break
    fi
done
