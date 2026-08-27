#!/usr/bin/env python3
"""Check that every derive operation actually does something.

These five operations have no automated coverage worth the name: they need a
real engine, real weights and a real song, so `cargo test` can only reach their
string handling. This exercises them for real, against a running engine, and
measures the output instead of trusting that a completed job did the work.

That distinction is the point. Every operation "succeeded" the first time this
was run, while stem extraction and add-instrument were returning near-copies of
their input — 55 seconds of compute for nothing. What exposed it was measuring
the residual against the source, and noticing two different stems were more
alike than either was to the mix.

    ./engine/check-derive.py <track-id>
    ./engine/check-derive.py <track-id> --reuse   # re-score existing renders

Reads the track from ~/Music/Aria and needs the engine on 127.0.0.1:8422 with
the SFT weights installed. Takes about five minutes.
"""
import json, pathlib, subprocess, sys, time, uuid, urllib.request

ACE = "http://127.0.0.1:8422"
MODELS = pathlib.Path.home() / ".local/share/org.aria.music/models"
LIB = pathlib.Path.home() / "Music/Aria"
OUT = pathlib.Path("/tmp/aria-derive-check")

# How much each operation should change the audio, as the level of
# (source - result) in dB. Higher means more changed.
#
# One threshold does not fit all of them, and assuming it did was the first
# version's mistake. A cover restyles everything and lands near -20 dB. A
# repaint rewrites fifteen seconds of a two-and-a-half minute track, so most of
# the file is untouched and it sits lower. An extend leaves the original
# completely alone and only adds to the end, so its residual is meaningless —
# length is the thing to check there.
EXPECTED_DB = {
    "cover": -30.0,
    "repaint": -45.0,
    "stem": -30.0,
    "add_layer": -30.0,
}


def residual_db(a, b):
    """How different two files are: the level of a minus b, in dB."""
    out = subprocess.run(
        ["ffmpeg", "-hide_banner", "-i", str(a), "-i", str(b), "-filter_complex",
         "[1:a]aeval=-val(0)|-val(1)[inv];[0:a][inv]amix=inputs=2:duration=shortest,volumedetect",
         "-f", "null", "/dev/null"],
        capture_output=True, text=True).stderr
    for line in out.splitlines():
        if "mean_volume:" in line:
            return float(line.split("mean_volume:")[1].strip().split()[0])
    return 0.0


def duration(path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration",
         "-of", "default=nw=1:nk=1", str(path)], capture_output=True, text=True)
    return float(out.stdout.strip() or 0)


def pick(pat):
    hits = sorted(p.name for p in MODELS.glob("*.gguf") if pat in p.name)
    if not hits:
        sys.exit(f"no model matching {pat!r} in {MODELS}")
    return hits[-1]


def submit(req, source_bytes, as_audio):
    b = uuid.uuid4().hex
    part = ("audio", "source.mp3", "audio/mpeg") if as_audio else \
           ("src_latents", "source.latent", "application/octet-stream")
    body = b""
    for name, fn, mime, data in [
            ("request", "request.json", "application/json", json.dumps(req).encode()),
            (*part, source_bytes)]:
        body += (f'--{b}\r\nContent-Disposition: form-data; name="{name}"; '
                 f'filename="{fn}"\r\nContent-Type: {mime}\r\n\r\n').encode() + data + b"\r\n"
    body += f"--{b}--\r\n".encode()
    r = urllib.request.Request(f"{ACE}/synth", data=body,
                               headers={"Content-Type": f"multipart/form-data; boundary={b}"})
    return json.loads(urllib.request.urlopen(r, timeout=120).read())["id"]


def collect(jid, dest):
    while True:
        st = json.loads(urllib.request.urlopen(f"{ACE}/job?id={jid}", timeout=30).read())["status"]
        if st == "done":
            break
        if st in ("failed", "cancelled"):
            return False
        time.sleep(5)
    raw = urllib.request.urlopen(f"{ACE}/job?id={jid}&result=1", timeout=300).read()
    sep = raw.split(b"\r\n", 1)[0]
    for p in raw.split(sep):
        if b"audio/" in p:
            dest.write_bytes(p.partition(b"\r\n\r\n")[2].rstrip(b"\r\n-"))
            return True
    return False


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("-")]
    reuse = "--reuse" in sys.argv
    if len(args) != 1:
        sys.exit(__doc__)
    track = args[0]
    src_mp3 = LIB / f"{track}.mp3"
    src_latent = LIB / f"{track}.latent"
    if not src_mp3.exists():
        sys.exit(f"no such track: {src_mp3}")
    OUT.mkdir(parents=True, exist_ok=True)
    src_len = duration(src_mp3)
    sft, turbo = pick("sft"), pick("turbo")
    caption = "An uplifting indie electronic track with arpeggiated synths and a driving beat."

    # kind -> (task_type, needs_sft, needs_audio_source, extra fields)
    ops = [
        ("cover",     "cover",   False, False, {"caption": "A slow acoustic folk ballad", "audio_cover_strength": 0.6}),
        ("repaint",   "repaint", False, False, {"repainting_start": 30.0, "repainting_end": 45.0}),
        ("extend",    "repaint", False, False, {"repainting_start": src_len, "repainting_end": src_len + 20}),
        ("stem",      "extract", True,  True,  {"track": "vocals", "lyrics": ""}),
        ("add_layer", "lego",    True,  True,  {"track": "guitar", "lyrics": "[Instrumental]"}),
    ]

    rows = []
    for name, task, sft_needed, wants_audio, extra in ops:
        req = {"synth_model": sft if sft_needed else turbo, "task_type": task,
               "caption": caption, "lyrics": "",
               "inference_steps": 50 if sft_needed else 8,
               "guidance_scale": 1.0, "shift": 1.0 if sft_needed else 3.0,
               "mp3_bitrate": 320, "peak_clip": 0, "audio_codes": ""}
        req.update(extra)
        use_audio = wants_audio or not src_latent.exists()
        payload = src_mp3.read_bytes() if use_audio else src_latent.read_bytes()

        dest = OUT / f"{name}.mp3"
        t = time.time()
        print(f"  {name:<10} task={task:<8} {'audio' if use_audio else 'latent':<7} ...",
              end="", flush=True)
        if reuse and dest.exists():
            ok, secs = True, 0.0
            print(" reused", end="")
        else:
            ok = collect(submit(req, payload, use_audio), dest)
            secs = time.time() - t
        if not ok:
            print(f" FAILED ({secs:.0f}s)")
            rows.append((name, "FAILED", 0.0, 0.0))
            continue
        rows.append((name, "ok", residual_db(src_mp3, dest), duration(dest)))
        print(f" {secs:.0f}s")

    print(f"\n  {'operation':<12}{'residual':>10}{'length':>10}   verdict")
    bad = 0
    for name, status, res, dur in rows:
        if status != "ok":
            print(f"  {name:<12}{'—':>10}{'—':>10}   FAILED")
            bad += 1
            continue
        if name == "extend":
            want = src_len + 20
            ok = abs(dur - want) < 2.0
            note = f"added {dur - src_len:.0f}s" if ok else f"WRONG LENGTH, wanted {want:.0f}s"
        else:
            ok = res > EXPECTED_DB[name]
            note = "changed the audio" if ok else "NO-OP — returned its input"
        bad += 0 if ok else 1
        print(f"  {name:<12}{res:>9.1f}dB{dur:>9.1f}s   {note}")
    if bad:
        print(f"\n  {bad} operation(s) did not do their job.")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
