#!/bin/bash

# Error monitoring script for Grandine ePBS development
# This script continuously checks for compilation errors and saves them to a file

ERROR_FILE="/tmp/grandine_errors.txt"
ERROR_JSON="/tmp/grandine_errors.json"
LAST_HASH="/tmp/last_error_hash"

echo "Starting error monitoring for Grandine..."
echo "Errors will be saved to: $ERROR_FILE"
echo "JSON format saved to: $ERROR_JSON"
echo "Press Ctrl+C to stop monitoring"

while true; do
    # Run cargo check with both formats
    echo "=== Checking at $(date) ===" > "$ERROR_FILE"
    
    # Short format for quick reading - capture both stdout and stderr
    cargo check --message-format=short >> "$ERROR_FILE" 2>&1
    
    # JSON format for detailed parsing
    cargo check --message-format=json > "$ERROR_JSON" 2>&1
    
    # Create a hash of the error file to detect changes
    NEW_HASH=$(md5sum "$ERROR_FILE" | cut -d' ' -f1)
    
    # Check if errors changed
    if [ -f "$LAST_HASH" ]; then
        OLD_HASH=$(cat "$LAST_HASH")
        if [ "$NEW_HASH" != "$OLD_HASH" ]; then
            echo "[$(date +%H:%M:%S)] Errors changed - check $ERROR_FILE"
        fi
    fi
    
    echo "$NEW_HASH" > "$LAST_HASH"
    
    # Wait 3 seconds before next check
    sleep 3
done