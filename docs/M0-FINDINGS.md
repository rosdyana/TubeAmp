# M0 spike findings (2026-11-14, macOS 26.7.2, wry 0.57, winit 0.30.13)

Spike: `spikes/m0` (scripted mode: `M0_SCRIPT=<videoId>`; env flags below).

| Check | Result |
|---|---|
| wry webview as child of a winit window, loading music.youtube.com (Safari UA) | ✅ |
| Read `#movie_player` metadata/time/state, `loadVideoById`, play/pause | ✅ |
| `navigator.mediaSession.metadata` populated by YTM | ✅ |
| Playback continues while host window hidden/minimized | ✅ (`document.hidden` is unreliable in a child webview, so don't depend on it) |
| Web Audio `createMediaElementSource` on YTM's normal MSE playback (WKWebView) | ❌ **silent**: WebKit's MSE player doesn't feed MediaElementAudioSourceNode (context runs, analyser reads exact zeros) |
| Same with MSE hidden (`M0_NOMSE`) → progressive googlevideo URL | ❌ silent (cross-origin taint) |
| …plus forcing `crossOrigin="anonymous"` | ✅ works, but only format 18 = **360p H.264 + ~96 kbps AAC**. Rejected for quality |
| **Core Audio process tap of our WebKit GPU process (`M0_TAP=1 M0_NOGRAPH=1`)** | ✅ **full-quality audio into Rust at 48 kHz; native EQ bit-transparent at flat, ~11 dB band shifts measured** |
| Google sign-in + session persistence | ⏳ manual check |
| Original output muted while tapped (no doubled audio) | ⏳ manual check (ear) |
| Now Playing / media keys interplay | ⏳ M6 |

## Decision
- **macOS:** YTM plays at full quality. `tap.m` taps every process attributed to us (bundled app: responsible pid == ours; unbundled `cargo run`: same responsible pid as us + `com.apple.WebKit.*` bundle ID). It uses `CATapMutedWhenTapped`, a private aggregate device (main subdevice = default output), and one IOProc that hands Rust the input and output buffers, so the DSP runs in-line. EQ, visualizer and Milkdrop PCM all come from that callback.
- **Windows (WebView2/Chromium):** the in-page Web Audio graph is expected to work on MSE. Verify when we first build on Windows; if it fails, fall back to process loopback.
- The EQ is a shared model (preamp + 10 bands); each backend consumes it.

## Gotchas for M1+
- The GPU process can be relaunched by WebKit (crash, memory pressure). The tap must watch `kAudioHardwarePropertyProcessObjectList` and re-target.
- Default output device changes (e.g. AirPods connecting) require rebuilding the aggregate device.
- Needs `NSAudioCaptureUsageDescription` in Info.plist. Under `cargo run`, the TCC prompt is attributed to the terminal.
- Boosting bands clips at ±1.0. The real chain needs Winamp-style preamp headroom plus a soft limiter.
- Set the Eq sample rate from the aggregate device *before* starting the IOProc (the spike swaps it after start, which is racy).

## Winamp EQ model (`crates/eq`, 2026-11-14)
- Bands 60/170/310/600/1k/3k/6k/12k/14k/16k. Slider values are 1..=64 (33 = 0 dB, 1 = -12, 64 = +12), the same scale as `.eqf` after its byte inversion.
- RBJ peaking biquads with Q = 1.41 (Winamp `eq10dsp.h`). Preamp ±12 dB, ramped per block. Balance. Soft limiter above -1 dBFS.
- The 17 built-in Winamp presets and the values come from Webamp's `presets/builtin.json` (MIT). `.eqf`/`winamp.q1` read and write.
- Live tap check: flat is bit-transparent. Full Bass moved low-vs-mid from about -8 dB to about -2 dB; Full Treble moved it to about -17 dB.
- Not verified against real Winamp: whether the 60 Hz/16 kHz bands should be shelves (Webamp uses shelves, while Winamp's header implies uniform peaking).
