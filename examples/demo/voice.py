#!/usr/bin/env python3
"""voice.py speak OUTDIR | mix LOG START OUTDIR VIDEO OUT: a Thai voice-over for joint_tutorial.py.

`speak` turns each caption's NARRATION into OUTDIR/voice/NN.mp3 with the edge-tts package
(`pip install edge-tts`; a neural Thai voice) and writes OUTDIR/voice/index.json
({caption: {"file", "seconds"}}) for joint_tutorial.py --voice, so each step waits for its words.
`mix` lays the clips onto VIDEO at the moments the captions appeared (from the --log and the
recording's start, as overlay.py does) and writes OUT."""

import asyncio
import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from joint_tutorial import NARRATION  # noqa: E402

VOICE = "th-TH-PremwadeeNeural"


def seconds(path):
    out = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path], capture_output=True, text=True, check=True)
    return float(out.stdout.strip())


async def one(words, f):
    """One clip: a neural Thai voice (edge-tts), then Google's (gTTS), then espeak-ng."""
    errors = []
    try:
        import edge_tts

        for voice, rate in ((VOICE, "-5%"), (VOICE, "+0%"), ("th-TH-NiwatNeural", "+0%")):
            for _ in range(2):
                try:
                    await edge_tts.Communicate(words, voice, rate=rate).save(f)
                    if os.path.getsize(f) > 1000:
                        return f"edge-tts {voice}"
                except Exception as e:  # noqa: BLE001 - try the next voice
                    errors.append(f"edge-tts {voice}: {e}")
                await asyncio.sleep(2)
    except ImportError as e:
        errors.append(str(e))
    try:
        from gtts import gTTS

        gTTS(words, lang="th").save(f)
        return "gTTS"
    except Exception as e:  # noqa: BLE001
        errors.append(f"gTTS: {e}")
    wav = f[:-4] + ".wav"
    if subprocess.run(["espeak-ng", "-v", "th", "-w", wav, words]).returncode == 0:
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", wav, f], check=True)
        return "espeak-ng"
    raise SystemExit("no Thai voice worked: " + "; ".join(errors))


async def speak(outdir):
    d = os.path.join(outdir, "voice")
    os.makedirs(d, exist_ok=True)
    index = {}
    for i, (caption, words) in enumerate(NARRATION.items()):
        f = os.path.join(d, f"{i:02}.mp3")
        how = await one(words, f)
        index[caption] = {"file": f, "seconds": seconds(f)}
        print(f"{index[caption]['seconds']:5.1f}s  [{how}]  {words}", flush=True)
    json.dump(index, open(os.path.join(d, "index.json"), "w"), ensure_ascii=False, indent=1)


def mix(log, start, outdir, video, out):
    index = json.load(open(os.path.join(outdir, "voice", "index.json")))
    clips = []
    for line in open(log):
        e = json.loads(line)
        c = e.get("caption")
        if c in index:
            clips.append((max(0.0, e["t"] - float(start)), index[c]["file"]))
    if not clips:
        raise SystemExit("no narrated captions in the log")
    args = ["ffmpeg", "-loglevel", "error", "-y", "-i", video]
    parts = []
    for i, (at, f) in enumerate(clips):
        args += ["-i", f]
        ms = int(at * 1000)
        parts.append(f"[{i + 1}:a]adelay={ms}|{ms},aresample=48000[a{i}]")
    graph = ";".join(parts) + ";" + "".join(f"[a{i}]" for i in range(len(clips))) + f"amix=inputs={len(clips)}:normalize=0,volume=1.6[a]"
    args += ["-filter_complex", graph, "-map", "0:v", "-map", "[a]", "-c:v", "copy", "-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart", out]
    subprocess.run(args, check=True)


if __name__ == "__main__":
    if sys.argv[1:2] == ["speak"]:
        asyncio.run(speak(sys.argv[2]))
    elif sys.argv[1:2] == ["mix"]:
        mix(*sys.argv[2:7])
    else:
        raise SystemExit(__doc__)
