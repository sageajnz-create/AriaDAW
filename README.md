# Aria

**Your music. Your machine. No limits.**

Aria makes complete songs — with real sung vocals — entirely on your own computer.

No account. No credits. No daily cap. No subscription. No queue.
Everything you make is yours, including for commercial use.

> Aria exists because music creation shouldn't be rationed. If you can't play an
> instrument, can't afford a studio, or can't afford a subscription, you should still
> be able to make the song in your head.

## How it's free

Aria isn't a service with a free tier — there's no server to meter. It runs an
MIT-licensed music model ([ACE-Step 1.5](https://github.com/ace-step/ACE-Step-1.5))
locally through [acestep.cpp](https://github.com/ServeurpersoCom/acestep.cpp).
Your computer does the work, so nobody can put a price on it later.

After the one-time model download, Aria works completely offline.

## What it does

- **Describe a song, get a song** — full track with sung lyrics, 50+ languages
- **Write your own lyrics** with structure tags (`[verse]`, `[chorus]`, `[bridge]`)
- **Control the music** — style, BPM, key, duration, seed
- **Rework what you made** — regenerate a section, extend a track, make a cover
- **Add an instrument** — a new part played over the song as it is
- **Split into stems** — vocals, drums, bass, other
- **Follow the words** — lyrics scroll with the song while it plays
- **Keep everything** — plain audio files in a folder you control

## Requirements

Any reasonably modern GPU works — AMD, Intel, or NVIDIA — via **Vulkan**, so there's
no vendor runtime (ROCm/CUDA) to install. CPU-only also works, just slower.

| | Minimum | Recommended |
|---|---|---|
| GPU VRAM | 4 GB (or CPU-only) | 8 GB+ |
| RAM | 8 GB | 16 GB |
| Disk | ~5 GB for models | ~10 GB |

**Linux audio note:** playback inside the app goes through WebKitGTK, which needs
GStreamer's `autodetect` plugin to find your speakers. Without it songs still
generate correctly and the files are fine — they just play silently in the window.
Aria detects this and tells you, but you can install it up front:

```bash
sudo pacman -S gst-plugins-good      # Arch / CachyOS
sudo apt install gstreamer1.0-plugins-good   # Debian / Ubuntu
```

## Status

In active development. See [PLAN.md](PLAN.md) for the architecture and roadmap.

## License

Aria is MIT licensed. It builds on ACE-Step 1.5 (MIT) and acestep.cpp (MIT).

Music you generate is yours. Aria claims no rights to your output.
