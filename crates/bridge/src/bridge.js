// TubeAmp bridge: injected into music.youtube.com by wry (main frame only).
//
// Reads the player and posts snapshots to Rust, and runs the commands Rust sends.
// Wire format: JSON objects tagged by "t", see crates/bridge/src/proto.rs.
//   page -> Rust: window.ipc.postMessage(JSON.stringify({t: "state" | "ended" | "error" | "ready" | "apiresult" | "key" | "escape", ...}))
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
//
// Media session: the page's navigator.mediaSession is shadowed below, so WebKit does not publish
// its own Now Playing entry or handle media keys itself. TubeAmp's entry (see nowplaying.rs) is the
// only one. The page's metadata is still read here, for the cover art in each state snapshot.
(() => {
  if (window.top !== window || window.__tubeamp) return;

  const post = (msg) => {
    try {
      window.ipc.postMessage(JSON.stringify(msg));
    } catch (_) {}
  };

  const shadowMediaSession = () => {
    const ms = navigator.mediaSession;
    if (!ms) return;
    let metadata = null;
    let playbackState = "none";
    try {
      Object.defineProperty(ms, "metadata", {
        configurable: true,
        get: () => metadata,
        set: (value) => {
          metadata = value;
        },
      });
      Object.defineProperty(ms, "playbackState", {
        configurable: true,
        get: () => playbackState,
        set: (value) => {
          playbackState = value;
        },
      });
      Object.defineProperty(ms, "setActionHandler", {
        configurable: true,
        value: () => {},
      });
    } catch (_) {}
  };
  shadowMediaSession();

  const artworkUrl = () => {
    const meta = navigator.mediaSession && navigator.mediaSession.metadata;
    const art = meta && meta.artwork;
    if (!art || !art.length) return null;
    const last = art[art.length - 1];
    return last && typeof last.src === "string" && last.src ? last.src : null;
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
    eventAt: 0,
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
      thumbnail: attempt(artworkUrl),
    };
  };

  const isPlaying = (ps) => ps === 1 || ps === 3;

  // Posts one snapshot now, and the end of a video once (player state 0 after something else).
  // Called by the timer and by the media events below, so the state does not depend on the timer
  // alone. A hidden webview runs its timers slowly, but the media events still arrive.
  const report = () => {
    const msg = snapshot(player(), Date.now());
    const ps = msg.ps;
    post(msg);
    // YouTube reports state 0 once when a video ends. Only fire on the transition into 0.
    if (ps !== null) {
      if (ps === 0 && T.lastPs !== null && T.lastPs !== 0) post({ t: "ended" });
      T.lastPs = ps;
    }
    return ps;
  };

  // Media events that change the state, and timeupdate, which is throttled to 250 ms.
  const MEDIA_EVENTS = ["play", "pause", "seeked", "loadedmetadata", "ended"];
  const TIMEUPDATE_MS = 250;

  // Errors from the <video> element. YouTube Music swaps the element, so re-attach when it changes.
  const watchVideo = () => {
    const v = document.querySelector("video");
    if (!v || T.video === v) return;
    T.video = v;
    v.addEventListener("error", () => {
      const e = v.error;
      post({ t: "error", where: "video", msg: e ? `code ${e.code}: ${e.message}` : "unknown" });
    });
    for (const name of MEDIA_EVENTS) {
      v.addEventListener(name, () => attempt(report));
    }
    v.addEventListener("timeupdate", () => {
      const now = Date.now();
      if (now - T.eventAt < TIMEUPDATE_MS) return;
      T.eventAt = now;
      attempt(report);
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
#tubeamp-art { display: none; }
html.tubeamp-video #tubeamp-art.on {
  display: block !important; visibility: visible !important;
  position: fixed !important; inset: 0 !important; z-index: 2147483600 !important;
  background-color: #000 !important; background-position: center !important;
  background-size: contain !important; background-repeat: no-repeat !important;
}
html.tubeamp-video #tubeamp-art.on::before {
  content: ""; position: absolute; inset: -40px;
  background-image: inherit; background-position: center; background-repeat: no-repeat;
  background-size: cover; filter: blur(28px) brightness(0.45); z-index: -1;
}
html.tubeamp-video #tubeamp-art.on::after {
  content: ""; position: absolute; inset: 0;
  background-image: inherit; background-position: center; background-repeat: no-repeat;
  background-size: contain;
}
`;

  // Audio-only items (YouTube Music "songs", MUSIC_VIDEO_TYPE_ATV) have no video frames, so the
  // video view would be black. Show the track's artwork instead, larger than the reported size.
  const bigArt = (url) => url.replace(/=w\d+-h\d+/, "=w1200-h1200");
  const applyArt = () => {
    const body = document.body;
    if (!body) return;
    let art = document.getElementById("tubeamp-art");
    if (!art) {
      art = document.createElement("div");
      art.id = "tubeamp-art";
      body.appendChild(art);
    }
    const video = document.querySelector("video");
    const noFrames = !video || !video.videoWidth;
    const url = attempt(artworkUrl);
    const show = !!(T.viewMode && noFrames && url);
    if (show) {
      const css = `url("${bigArt(url).replace(/"/g, "%22")}")`;
      if (art.dataset.src !== css) {
        art.style.backgroundImage = css;
        art.dataset.src = css;
      }
    }
    art.classList.toggle("on", show);
  };

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
    applyArt();
  };

  // Escape goes to the page while the webview has focus. Rust uses it to leave fullscreen.
  document.addEventListener(
    "keydown",
    (e) => {
      if (e.key === "Escape") post({ t: "escape" });
    },
    true
  );

  // ---- Keys in the video view ------------------------------------------------------------------

  // The shortcuts TubeAmp runs (crates/app/src/hotkeys.rs, PAGE_KEYS). A key in this list is
  // also prevented from reaching the page. Any other key still goes to the page, so YouTube
  // Music's own shortcuts keep working. Keep this list in step with PAGE_KEYS (an app test checks it).
  const PLAIN_KEYS = ["arrowleft", "arrowright", "arrowup", "arrowdown", "z", "x", "c", "v", "b", "l"];
  const CTRL_KEYS = ["d", "t", "l", "m"];
  const ALT_KEYS = ["l", "v"];

  // The key's letter: under Alt or Ctrl on macOS, KeyboardEvent.key is a symbol, so letters come
  // from `code` (KeyV), which does not depend on modifiers or the layout's shifted characters.
  const keyName = (e) => {
    if (e.code && /^Key[A-Z]$/.test(e.code)) return e.code.slice(3).toLowerCase();
    return String(e.key || "").toLowerCase();
  };

  // Whether TubeAmp runs a shortcut for this key press. Only these are prevented.
  // The same rule as shortcut_for in hotkeys.rs: Cmd, and Ctrl+Alt, are never shortcuts; Ctrl is
  // a Ctrl shortcut only; Alt takes its own letters, and otherwise falls through to the plain keys.
  const handledByTubeAmp = (e) => {
    const k = keyName(e);
    if (e.metaKey || (e.ctrlKey && e.altKey)) return false;
    if (e.ctrlKey) return CTRL_KEYS.includes(k);
    if (e.altKey && ALT_KEYS.includes(k)) return true;
    return PLAIN_KEYS.includes(k);
  };

  const isEditable = (el) => {
    if (!el || !el.tagName) return false;
    const tag = el.tagName.toLowerCase();
    return tag === "input" || tag === "textarea" || tag === "select" || !!el.isContentEditable;
  };

  // In the video view the webview can have focus (after a click in the video). Keys then come
  // here first: forward them to Rust, and prevent the ones TubeAmp handles. Escape is forwarded
  // by the listener above, so it is left out here. Nothing is forwarded in the browser view.
  document.addEventListener(
    "keydown",
    (e) => {
      if (!T.viewMode || e.isComposing || e.key === "Escape") return;
      if (isEditable(e.target)) return;
      const handled = handledByTubeAmp(e);
      post({
        t: "key",
        key: e.key,
        code: e.code || "",
        ctrl: !!e.ctrlKey,
        alt: !!e.altKey,
        shift: !!e.shiftKey,
        meta: !!e.metaKey,
        repeat: !!e.repeat,
      });
      if (handled) e.preventDefault();
    },
    true
  );

  const tick = () => {
    let ps = null;
    try {
      applyView();
      watchVideo();
      ps = report();
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
        case "cue":
          p.cueVideoById(c.video_id, Number(c.secs) || 0);
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
