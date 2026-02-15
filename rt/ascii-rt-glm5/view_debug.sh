#!/bin/bash
# View debug frames with ANSI colors
# Usage: ./view_debug.sh [frame_number]

FRAME=${1:-0}
FRAME_FILE=$(printf "debug/frame_%03d.txt" "$FRAME")

if [ -f "$FRAME_FILE" ]; then
    cat "$FRAME_FILE"
else
    echo "Frame $FRAME_FILE not found. Run with --debug first."
fi
