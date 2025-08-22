#!/bin/bash

# One-time error check script - run only when needed
# Usage: ./check_errors_once.sh [package]

ERROR_FILE="/tmp/grandine_errors.txt"

if [ "$1" == "-h" ] || [ "$1" == "--help" ]; then
    echo "Usage: ./check_errors_once.sh [package]"
    echo "  No args: Check entire project"
    echo "  With package: Check specific package (e.g., types)"
    exit 0
fi

echo "Running cargo check..."

if [ -z "$1" ]; then
    # Check entire project
    cargo check --message-format=short 2>&1 | tee "$ERROR_FILE"
else
    # Check specific package
    cargo check -p "$1" --message-format=short 2>&1 | tee "$ERROR_FILE"
fi

echo ""
echo "Errors saved to: $ERROR_FILE"
echo "Total error count: $(grep -c "error\[E" "$ERROR_FILE" 2>/dev/null || echo "0")"