#!/bin/bash

while true; do
    if [ -e /var/run/cryptpilot/.network-online ]; then
        break
    fi
    sleep 1
done
