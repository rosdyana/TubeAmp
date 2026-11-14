// TubeAmp bridge: injected into music.youtube.com by wry (main frame only).
//
// Reads the player and posts snapshots to Rust, and runs the commands Rust sends.
// Wire format: JSON objects tagged by "t", see crates/bridge/src/proto.rs.
//   page -> Rust: window.ipc.postMessage(JSON.stringify({t: "state" | "ended" | "error" | "ready" | "apiresult", ...}))
//   Rust -> page: window.__tubeamp.cmd({t: "play" | "pause" | "api" | ..., ...})
//
// InnerTube: {t: "api", req, endpoint, body} runs a same-origin POST to /youtubei/v1/<endpoint>
// from inside the page, so the page's cookies and ytcfg apply. The page posts {t: "ready",
// signed_in} once ytcfg has INNERTUBE_CONTEXT, and requests sent before that wait in a queue.
// The answer is {t: "apiresult", req, ok, status, body}, with the raw response text in body.
//
// View: {t: "setviewmode", video} switches between the video view (only the player's <video>
// fills the webview) and the full page. The video view is applied only while a player exists, so
// a page without one (sign-in, say) always shows in full. See applyView below.
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
    viewMode: true,
    keepRoot: null,
    markedAt: 0,
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

  // ---- View ------------------------------------------------------------------------------------

  // The video view hides everything with `visibility` (keeps layout, so nothing reflows), then
  // shows the player's subtree and its ancestors again. Those are marked with KEEP. The rule for
  // hidden elements is `body *`, so an element inside a marked subtree is shown only if it is
  // marked too, which markView does. Only the outline of the page is marked, not its siblings.
  const KEEP = "data-tubeamp-keep";
  const VIEW_STYLE_ID = "tubeamp-view-style";
  const VIEW_CSS = `
html.tubeamp-video,
html.tubeamp-video body { background: #000 !important; overflow: hidden !important; }
html.tubeamp-video body * { visibility: hidden !important; }
html.tubeamp-video [${KEEP}] { visibility: visible !important; background: transparent !important; }
html.tubeamp-video #movie_player,
html.tubeamp-video video {
  position: fixed !important; top: 0 !important; left: 0 !important;
  width: 100vw !important; height: 100vh !important;
  max-width: none !important; max-height: none !important;
  margin: 0 !important; transform: none !important;
  z-index: 2147483000 !important; background: #000 !important;
}
html.tubeamp-video video { object-fit: contain !important; }
`;

  const parentOf = (el) => el.parentElement || (el.parentNode && el.parentNode.host) || null;

  const clearView = () => {
    for (const el of document.querySelectorAll(`[${KEEP}]`)) el.removeAttribute(KEEP);
  };

  const markView = (root) => {
    clearView();
    for (let el = root; el && el !== document.documentElement; el = parentOf(el)) {
      el.setAttribute(KEEP, "");
    }
    for (const el of root.querySelectorAll("*")) el.setAttribute(KEEP, "");
    root.setAttribute(KEEP, "");
  };

  // Brings the page in line with T.viewMode. Cheap enough for every tick: it re-marks the player
  // when the player element changes, and at least once a second.
  const applyView = () => {
    const root = document.documentElement;
    if (!root) return;
    if (!document.getElementById(VIEW_STYLE_ID)) {
      const style = document.createElement("style");
      style.id = VIEW_STYLE_ID;
      style.textContent = VIEW_CSS;
      (document.head || root).appendChild(style);
    }
    const keepRoot = T.viewMode
      ? document.getElementById("movie_player") || document.querySelector("video")
      : null;
    if (keepRoot) {
      const now = Date.now();
      if (keepRoot !== T.keepRoot || now - T.markedAt > 1000) {
        markView(keepRoot);
        T.keepRoot = keepRoot;
        T.markedAt = now;
      }
    } else if (T.keepRoot) {
      clearView();
      T.keepRoot = null;
    }
    root.classList.toggle("tubeamp-video", !!keepRoot);
  };

  // Escape goes to the page while the webview has focus. Rust uses it to leave fullscreen.
  document.addEventListener(
    "keydown",
    (e) => {
      if (e.key === "Escape") post({ t: "escape" });
    },
    true
  );

  const tick = () => {
    let ps = null;
    try {
      applyView();
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

  // ---- InnerTube requests ----------------------------------------------------------------------

  const ytcfgGet = (key) => {
    try {
      return window.ytcfg && window.ytcfg.get ? window.ytcfg.get(key) : null;
    } catch (_) {
      return null;
    }
  };

  // Value of a cookie by exact name, or null.
  const cookie = (name) => {
    for (const part of document.cookie.split(";")) {
      const t = part.trim();
      if (t.startsWith(name + "=")) return t.slice(name.length + 1);
    }
    return null;
  };

  const sha1Hex = async (text) => {
    const digest = await crypto.subtle.digest("SHA-1", new TextEncoder().encode(text));
    return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
  };

  // The web client's Authorization header: SAPISIDHASH <ts>_<sha1("<ts> <SAPISID> <origin>")>.
  // Null when the SAPISID cookie is not readable, which means logged-out results.
  const authorization = async (origin) => {
    const sid = cookie("SAPISID") || cookie("__Secure-3PAPISID");
    if (!sid) return null;
    const ts = Math.floor(Date.now() / 1000);
    return `SAPISIDHASH ${ts}_${await sha1Hex(`${ts} ${sid} ${origin}`)}`;
  };

  const runApi = async (c) => {
    const origin = location.origin;
    let status = 0;
    try {
      const headers = {
        "Content-Type": "application/json",
        "X-Goog-AuthUser": "0",
        "X-Origin": origin,
      };
      const auth = await authorization(origin);
      if (auth) headers["Authorization"] = auth;
      const body = { context: ytcfgGet("INNERTUBE_CONTEXT"), ...(c.body || {}) };
      const res = await fetch(`/youtubei/v1/${c.endpoint}?prettyPrint=false`, {
        method: "POST",
        headers,
        credentials: "same-origin",
        body: JSON.stringify(body),
      });
      status = res.status;
      const text = await res.text();
      post({ t: "apiresult", req: c.req, ok: res.ok, status, body: text });
    } catch (e) {
      post({ t: "apiresult", req: c.req, ok: false, status, body: String(e) });
    }
  };

  const api = { ready: false, queue: [] };

  const request = (c) => {
    if (api.ready) runApi(c);
    else api.queue.push(c);
  };

  const waitForYtcfg = () => {
    if (!ytcfgGet("INNERTUBE_CONTEXT")) {
      setTimeout(waitForYtcfg, 500);
      return;
    }
    api.ready = true;
    post({ t: "ready", signed_in: !!ytcfgGet("LOGGED_IN") });
    for (const c of api.queue.splice(0)) runApi(c);
  };

  T.cmd = (c) => {
    if (c.t === "api") {
      request(c);
      return;
    }
    if (c.t === "setviewmode") {
      T.viewMode = !!c.video;
      try {
        applyView();
      } catch (e) {
        post({ t: "error", where: "cmd:setviewmode", msg: String(e) });
      }
      return;
    }
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
  waitForYtcfg();
})();
