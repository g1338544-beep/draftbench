#!/bin/bash
# record_tutorial.sh DISPLAY PORT OUTDIR DESIGN: record joint_tutorial.py driving the app on X
# display DISPLAY (started with --control PORT) to OUTDIR/joint-tutorial.mp4, with captions and a
# drawn pointer (overlay.py), as record_ui.sh does for the enclosure demo. With OUTDIR/voice/index.json
# (voice.py speak) the steps wait for their narration and the Thai voice-over is mixed in.
set -e
DISP=${1:-:99}; PORT=${2:-7878}; OUT=$(cd "${3:-.}" && pwd); DESIGN=$4
HERE=$(cd "$(dirname "$0")" && pwd)
python3 - "$HERE" "$PORT" <<'PY'
import sys,time
sys.path.insert(0,sys.argv[1])
from enclosure_mcp import control
control(int(sys.argv[2]),"ui.resize",{"width":1920,"height":1080})
time.sleep(2)
PY
ffmpeg -loglevel info -nostats -y -f x11grab -draw_mouse 0 -video_size 1920x1080 -framerate 30 -i "$DISP" \
  -c:v libx264 -preset veryfast -crf 12 -pix_fmt yuv420p "$OUT/raw.mp4" 2> "$OUT/ffmpeg.log" &
FF=$!
trap 'kill -INT $FF 2>/dev/null || true; wait $FF 2>/dev/null || true' EXIT
sleep 1
VOICE=(); [ -f "$OUT/voice/index.json" ] && VOICE=(--voice "$OUT/voice/index.json")
python3 "$HERE/joint_tutorial.py" --port "$PORT" --design "$DESIGN" --log "$OUT/steps.log" "${VOICE[@]}"
sleep 1
kill -INT $FF; wait $FF || true
trap - EXIT
START=$(grep -o "start: [0-9.]*" "$OUT/ffmpeg.log" | head -1 | cut -d" " -f2)
python3 "$HERE/overlay.py" "$OUT/steps.log" "$START" "$OUT"
read CX CY < "$OUT/cursor.start"
ffmpeg -loglevel error -y -i "$OUT/raw.mp4" -loop 1 -i "$OUT/cursor.png" -loop 1 -i "$OUT/ripple.png" -filter_complex \
  "[0:v]ass=$OUT/captions.ass[v0];[v0][2:v]overlay@rip=x=-100:y=-100:enable='$(cat "$OUT/ripple.enable")':shortest=1[v1];[v1][1:v]overlay@cur=x=$CX:y=$CY:shortest=1,sendcmd=f=$OUT/cursor.cmd[v]" \
  -map "[v]" -c:v libx264 -preset slow -crf 16 -pix_fmt yuv420p -movflags +faststart "$OUT/joint-tutorial.mp4"
if [ -f "$OUT/voice/index.json" ]; then
  mv "$OUT/joint-tutorial.mp4" "$OUT/joint-tutorial-silent.mp4"
  python3 "$HERE/voice.py" mix "$OUT/steps.log" "$START" "$OUT" "$OUT/joint-tutorial-silent.mp4" "$OUT/joint-tutorial.mp4"
fi
ls -la "$OUT"/joint-tutorial*.mp4
