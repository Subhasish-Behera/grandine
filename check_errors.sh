#!/bin/bash

# Helper script to check errors in specific parts of the codebase
# Usage: ./check_errors.sh [filename] [line_number]

ERROR_FILE="/tmp/grandine_errors.txt"

if [ $# -eq 0 ]; then
    echo "=== All current errors ==="
    cat "$ERROR_FILE" 2>/dev/null || echo "No errors found. Make sure monitor_errors.sh is running."
elif [ $# -eq 1 ]; then
    echo "=== Errors in $1 ==="
    grep "$1" "$ERROR_FILE" 2>/dev/null || echo "No errors found in $1"
elif [ $# -eq 2 ]; then
    echo "=== Errors around $1:$2 ==="
    grep "$1:$2" "$ERROR_FILE" 2>/dev/null || echo "No errors found at $1:$2"
    echo ""
    echo "=== All errors in $1 ==="
    grep "$1" "$ERROR_FILE" 2>/dev/null
fi