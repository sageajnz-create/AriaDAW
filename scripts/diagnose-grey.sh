#!/bin/bash
# Capture what Aria is doing *while the window is grey*.
#
# Four rendering fixes have not settled this, so the next step is evidence from
# the failure itself rather than another guess. Run this while the window is
# grey and paste the output.
#
# It distinguishes the three possibilities:
#   - main thread blocked  -> thread states and wchan show a blocking syscall
#   - web process died     -> the child is gone, or a fresh core exists
#   - JavaScript error     -> nothing looks wrong here; the UI error boundary
#                             should be showing a message instead

set -u
ARIA=$(pgrep -x aria | head -1)
if [ -z "$ARIA" ]; then
    echo "Aria isn't running."
    exit 1
fi

echo "=== aria pid $ARIA ==="
ps -o pid,stat,%cpu,%mem,etime,wchan:24 -p "$ARIA"

echo
echo "=== its threads (D = uninterruptible I/O, R = running, S = sleeping) ==="
for t in /proc/$ARIA/task/*; do
    tid=$(basename "$t")
    printf "  tid %-8s %-4s %-22s %s\n" \
        "$tid" \
        "$(awk '{print $3}' "$t/stat" 2>/dev/null)" \
        "$(cat "$t/comm" 2>/dev/null)" \
        "$(cat "$t/wchan" 2>/dev/null)"
done

echo
echo "=== child processes ==="
pgrep -P "$ARIA" 2>/dev/null | while read -r c; do
    printf "  %-8s %-24s %s\n" "$c" "$(cat /proc/$c/comm 2>/dev/null)" \
        "$(ps -o %cpu=,stat= -p "$c" 2>/dev/null | tr -s ' ')"
done
[ -z "$(pgrep -P "$ARIA" 2>/dev/null)" ] && echo "  (none — the web process is gone)"

echo
echo "=== crashes in the last 15 minutes ==="
coredumpctl list --since "15 min ago" 2>/dev/null | tail -5 || echo "  (none recorded)"

echo
echo "=== engine ==="
if curl -sf --max-time 3 http://127.0.0.1:8422/health >/dev/null 2>&1; then
    echo "  responding"
else
    echo "  not responding"
fi

echo
echo "=== memory ==="
free -h | head -2
echo
echo "=== GPU ==="
echo "  VRAM $(( $(cat /sys/class/drm/card1/device/mem_info_vram_used 2>/dev/null || echo 0) / 1024 / 1024 )) MB"
