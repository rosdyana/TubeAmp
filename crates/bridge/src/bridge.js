// TubeAmp bridge: injected into music.youtube.com by wry (main frame only).
//
// Reads the player and posts snapshots to Rust, and runs the commands Rust sends.
// Wire format: JSON objects tagged by "t", see crates/bridge/src/proto.rs.
//   page -> Rust: window.ipc.postMessage(JSON.stringify({t: "state" | "ended" | "error", ...}))
//   Rust -> page: window.__tubeamp.cmd({t: "play" | "pause" | ..., ...})
//
// Audio is not handled here on macOS: the EQ and the analyser run in Rust on a Core Audio tap of
// the WebKit process. There is no Web Audio graph in this file.
(() => {
  if (window.top !== window || window.__tubeamp) return;

  const post = (msg) => {
    try {
      window.ipc.postMessage(JSON.stringify(msg));
    } catch (_) {}
  };

  const T = (window.__tubeamp = {
    cmd: null,
    lastPs: null,
    stats: { kbps: null, codec: null },
    statsAt: 0,
    video: null,
    timer: null,
  });

  // TODO(windows): attach the Web Audio EQ graph here, once the Windows backend lands:
  //   createMediaElementSource(video) -> preamp gain -> 10 peaking biquads -> balance
  //   -> analyser -> destination. macOS never calls this, because the Core Audio tap does the DSP.
  // function attachAudioGraph(video) {}

  const player = () => document.getElementById("movie_player");

  const attempt = (fn) => {
    try {
      return fn();
    } catch (_) {
      return null;
    }
  };

  // Nominal bitrate (kbps) of YouTube's audio itags. The stats-for-nerds object does not report
  // the audio bitrate, only the connection bandwidth, so this is the bitrate of the format.
  const AUDIO_KBPS = { 139: 48, 140: 128, 141: 256, 249: 50, 250: 70, 251: 160 };

  // Best-effort codec and bitrate from the player's stats-for-nerds object. The field names are
  // not a documented API. The audio entry in `codecs` looks like "avc1.4D401E (134) / opus (251)".
  // Anything that does not match gives null, and the window falls back to 128 kbps while playing.
  const readStats = (p) => {
    const st = attempt(() => (p.getStatsForNerds ? p.getStatsForNerds() : null));
    const codecs = st && typeof st.codecs === "string" ? st.codecs : "";
    const m = /(opus|vorbis|mp4a(?:\.\d+(?:\.\d+)?)?)\s*\((\d+)\)/i.exec(codecs);
    if (!m) return { kbps: null, codec: null };
    const name = m[1].toLowerCase();
    return {
      kbps: AUDIO_KBPS[Number(m[2])] || null,
      codec: name.startsWith("mp4a") ? "aac" : name,
    };
  };

  const snapshot = (p, now) => {
    if (now - T.statsAt >= 1000) {
      T.stats = p ? readStats(p) : { kbps: null, codec: null };
      T.statsAt = now;
    }
    const data = attempt(() => (p && p.getVideoData ? p.getVideoData() : null));
    return {
      t: "state",
      video_id: (data && data.video_id) || null,
      title: (data && data.title) || null,
      author: (data && data.author) || null,
      cur: attempt(() => p.getCurrentTime()),
      dur: attempt(() => p.getDuration()),
      ps: attempt(() => p.getPlayerState()),
      kbps: T.stats.kbps,
      codec: T.stats.codec,
    };
  };

  const isPlaying = (ps) => ps === 1 || ps === 3;

  // Errors from the <video> element. YouTube Music swaps the element, so re-attach when it changes.
  const watchVideo = () => {
    const v = document.querySelector("video");
    if (!v || T.video === v) return;
    T.video = v;
    v.addEventListener("error", () => {
      const e = v.error;
      post({ t: "error", where: "video", msg: e ? `code ${e.code}: ${e.message}` : "unknown" });
    });
  };

  const tick = () => {
    let ps = null;
    try {
      watchVideo();
      const msg = snapshot(player(), Date.now());
      ps = msg.ps;
      post(msg);
      // YouTube reports state 0 once when a video ends. Only fire on the transition into 0.
      if (ps !== null) {
        if (ps === 0 && T.lastPs !== null && T.lastPs !== 0) post({ t: "ended" });
        T.lastPs = ps;
      }
    } catch (e) {
      post({ t: "error", where: "tick", msg: String(e) });
    } finally {
      T.timer = setTimeout(tick, isPlaying(ps) ? 250 : 1000);
    }
  };

  T.cmd = (c) => {
    const p = player();
    if (!p) return;
    try {
      switch (c.t) {
        case "play":
          p.playVideo();
          break;
        case "pause":
          p.pauseVideo();
          break;
        case "toggle":
          if (p.getPlayerState() === 1) p.pauseVideo();
          else p.playVideo();
          break;
        case "stop":
          p.pauseVideo();
          p.seekTo(0, true);
          break;
        case "next":
          p.nextVideo();
          break;
        case "prev":
          if (p.getCurrentTime() > 3) p.seekTo(0, true);
          else p.previousVideo();
          break;
        case "seek":
          p.seekTo(Number(c.secs) || 0, true);
          break;
        case "volume":
          p.setVolume(Math.max(0, Math.min(100, Number(c.level) || 0)));
          break;
        case "load":
          p.loadVideoById(c.video_id);
          break;
        default:
          post({ t: "error", where: "cmd", msg: "unknown command " + c.t });
      }
    } catch (e) {
      post({ t: "error", where: "cmd:" + c.t, msg: String(e) });
    }
  };

  tick();
})();
