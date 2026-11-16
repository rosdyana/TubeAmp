// TubeAmp bridge: injected into music.youtube.com by wry (main frame only).
//
// Reads the player and posts snapshots to Rust, and runs the commands Rust sends.
// Wire format: JSON objects tagged by "t", see crates/bridge/src/proto.rs.
//   page -> Rust: window.ipc.postMessage(JSON.stringify({t: "state" | "ended" | "error" | "ready" | "apiresult" | "key" | "escape" | "samples", ...}))
//   Rust -> page: window.__tubeamp.cmd({t: "play" | "pause" | "api" | "eq" | ..., ...})
//
// InnerTube: {t: "api", req, endpoint, body} runs a same-origin POST to /youtubei/v1/<endpoint>
// from inside the page, so the page's cookies and ytcfg apply. The page posts {t: "ready",
// signed_in} once ytcfg has INNERTUBE_CONTEXT, and requests sent before that wait in a queue.
// The answer is {t: "apiresult", req, ok, status, body}, with the raw response text in body.
//
// View: {t: "setviewmode", video} switches between the video view (only the player's <video>
// fills the webview) and the full page. The video view is applied only while a player exists, so
// a page without one (sign-in, say) always shows in full. See applyView below.
// Ads: {t: "skipads", on} makes the page report YouTube's own Skip button when it is offered, as
// {t: "adskip", x, y} (Rust then sends a trusted click there); state snapshots carry `ad`.
//
// Audio is not handled here on macOS: the EQ and the analyser run in Rust on a Core Audio tap of
// the WebKit process, and no Web Audio graph is built. Elsewhere (the prelude sets
// __tubeamp_page_audio) this file runs the EQ as a Web Audio graph on the video element, and posts
// the post-EQ samples as {t: "samples", rate, data} for the analyser. {t: "eq", ...} sets the EQ.
// See ensureGraph below.
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
  // The page's media session. On macOS (the prelude sets `__tubeamp_native_media`) TubeAmp owns
  // it: the page's handlers and metadata are swallowed, and the bridge registers its own handlers
  // and sets the metadata, the playback state and the position. WebKit's Now Playing entry is then
  // the one TubeAmp shows, and the media keys go to TubeAmp's playlist. Elsewhere the page's
  // session is only shadowed, as before.
  const media = {
    native: null,
    ms: null,
    page: null,
    meta: null,
    state: null,
    hasTrack: false,
  };

  const MEDIA_ACTIONS = [
    "play",
    "pause",
    "stop",
    "previoustrack",
    "nexttrack",
    "seekto",
    "seekbackward",
    "seekforward",
  ];

  // Returns the native setters and methods of the session, once the page's are swallowed. Null
  // when this WebKit has no such session, so the page's session is only shadowed then.
  const takeMediaSession = (ms) => {
    const proto = Object.getPrototypeOf(ms);
    const own = (name) => Object.getOwnPropertyDescriptor(proto, name);
    const metadata = own("metadata");
    const playbackState = own("playbackState");
    if (
      !metadata ||
      !metadata.set ||
      !playbackState ||
      !playbackState.set ||
      typeof proto.setActionHandler !== "function" ||
      typeof proto.setPositionState !== "function"
    ) {
      return null;
    }
    const native = {
      metadata: metadata.set,
      playbackState: playbackState.set,
      setActionHandler: proto.setActionHandler,
      setPositionState: proto.setPositionState,
    };
    // The page's own metadata is kept (for the cover art) but not sent to the session.
    Object.defineProperty(ms, "metadata", {
      configurable: true,
      get: () => media.meta,
      set: (value) => {
        media.page = value;
      },
    });
    Object.defineProperty(ms, "playbackState", {
      configurable: true,
      get: () => media.state || "none",
      set: () => {},
    });
    Object.defineProperty(ms, "setActionHandler", { configurable: true, value: () => {} });
    Object.defineProperty(ms, "setPositionState", { configurable: true, value: () => {} });
    for (const action of MEDIA_ACTIONS) {
      try {
        native.setActionHandler.call(ms, action, (details) => {
          const d = details || {};
          const seek =
            typeof d.seekTime === "number"
              ? d.seekTime
              : typeof d.seekOffset === "number"
                ? d.seekOffset
                : null;
          post({ t: "media", action, seek_time: seek });
        });
      } catch (_) {}
    }
    return native;
  };

  const sessionNow = navigator.mediaSession;
  if (sessionNow) {
    media.ms = sessionNow;
    media.native = window.__tubeamp_native_media === true ? takeMediaSession(sessionNow) : null;
    if (!media.native) shadowMediaSession();
  }

  // The page's own metadata, for the cover art: the one the page set (native), or the shadow's.
  const pageMetadata = () =>
    media.native ? media.page : navigator.mediaSession && navigator.mediaSession.metadata;

  const artworkUrl = () => {
    const meta = pageMetadata();
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
    skipAds: false,
    skipAt: 0,
    adCovered: false,
    adObserver: null,
    keepRoot: null,
    markedAt: 0,
    eventAt: 0,
    audio: {
      ctx: null,
      input: null,
      filters: null,
      split: null,
      gl: null,
      gr: null,
      merge: null,
      limiter: null,
      tap: null,
      sources: new WeakSet(),
      eq: null,
      lastErr: null,
    },
  });

  // ---- Web Audio EQ (not macOS) ----------------------------------------------------------------
  // Off macOS (the prelude sets __tubeamp_page_audio) the EQ is a Web Audio graph on the video
  // element, with the chain of dsp.rs:
  //   video -> preamp gain -> 10 peaking biquads -> balance -> soft limiter -> destination
  // A ScriptProcessor after the limiter copies the post-EQ samples out for Rust (`samples`), which
  // feeds the analyser and Milkdrop. macOS never builds this graph: the Core Audio tap does the DSP.
  const pageAudio = window.__tubeamp_page_audio === true;
  // The bands of tubeamp_eq::BANDS_HZ, and dsp.rs's BAND_Q and LIMIT_KNEE.
  const EQ_FREQS = [60, 170, 310, 600, 1000, 3000, 6000, 12000, 14000, 16000];
  const EQ_Q = 1.41;
  const LIMIT_KNEE = 0.89;

  // dsp.rs's soft limiter, sampled over -1..1 (the WaveShaper's input range): the identity up to
  // the knee, then it approaches 1 without reaching it, and keeps the sign.
  const limiterCurve = () => {
    const n = 2049;
    const k = 1 - LIMIT_KNEE;
    const curve = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      const x = (i / (n - 1)) * 2 - 1;
      const a = Math.abs(x);
      const y = a <= LIMIT_KNEE ? a : Math.min(LIMIT_KNEE + k * Math.tanh((a - LIMIT_KNEE) / k), 1);
      curve[i] = x < 0 ? -y : y;
    }
    return curve;
  };

  // Builds the graph once. T.audio.ctx is set last, so it is only set for a complete graph.
  const ensureGraph = () => {
    const audio = T.audio;
    if (audio.ctx) return;
    const ctx = new AudioContext();
    const input = ctx.createGain();
    // Always stereo from here on, so a mono track still reaches both sides of the balance split.
    input.channelCount = 2;
    input.channelCountMode = "explicit";
    input.channelInterpretation = "speakers";
    const filters = EQ_FREQS.map((frequency) => {
      const f = ctx.createBiquadFilter();
      f.type = "peaking";
      f.frequency.value = frequency;
      f.Q.value = EQ_Q;
      f.gain.value = 0;
      return f;
    });
    const split = ctx.createChannelSplitter(2);
    const gl = ctx.createGain();
    const gr = ctx.createGain();
    const merge = ctx.createChannelMerger(2);
    const limiter = ctx.createWaveShaper();
    limiter.curve = limiterCurve();
    limiter.oversample = "none";

    input.connect(filters[0]);
    for (let i = 1; i < filters.length; i++) filters[i - 1].connect(filters[i]);
    filters[filters.length - 1].connect(split);
    split.connect(gl, 0);
    split.connect(gr, 1);
    gl.connect(merge, 0, 0);
    gr.connect(merge, 0, 1);
    merge.connect(limiter);
    limiter.connect(ctx.destination);

    // Blocks of 1024 frames, driven by the audio clock, so Rust gets a gap-free stream. Sent only
    // while the video plays. The mute gain keeps the node pulled without adding sound.
    const tap = ctx.createScriptProcessor(1024, 2, 1);
    tap.onaudioprocess = (e) => {
      const v = T.video;
      if (!v || v.paused) return;
      const buf = e.inputBuffer;
      const l = buf.getChannelData(0);
      const r = buf.numberOfChannels > 1 ? buf.getChannelData(1) : l;
      const data = new Array(l.length);
      for (let i = 0; i < l.length; i++) {
        data[i] = Math.round(((l[i] + r[i]) / 2) * 10000) / 10000;
      }
      post({ t: "samples", rate: ctx.sampleRate, data });
    };
    limiter.connect(tap);
    const mute = ctx.createGain();
    mute.gain.value = 0;
    tap.connect(mute);
    mute.connect(ctx.destination);

    Object.assign(audio, { ctx, input, filters, split, gl, gr, merge, limiter, tap });
  };

  const resumeAudio = () => {
    const ctx = T.audio.ctx;
    if (ctx) ctx.resume().catch(() => {});
  };

  // Posts an audio failure once per message, so a repeated failure does not flood the log.
  const audioError = (e) => {
    const msg = String(e);
    if (msg === T.audio.lastErr) return;
    T.audio.lastErr = msg;
    post({ t: "error", where: "audio", msg });
  };

  // Routes the element through the graph. The element is only rerouted once the source exists,
  // so when this fails the element plays as it did before.
  const attachAudio = (v) => {
    const audio = T.audio;
    if (!pageAudio || audio.sources.has(v)) return;
    try {
      ensureGraph();
      audio.ctx.createMediaElementSource(v).connect(audio.input);
      audio.sources.add(v);
      v.addEventListener("play", resumeAudio);
      resumeAudio();
      applyEq();
    } catch (e) {
      audioError(e);
    }
  };

  // The last EQ command as the graph's gains. Off means flat filters and unity preamp, as in
  // dsp.rs. Balance applies either way, as it does there.
  const applyEq = () => {
    const audio = T.audio;
    if (!audio.ctx || !audio.eq) return;
    const c = audio.eq;
    const t = audio.ctx.currentTime;
    const glide = (param, value) => param.setTargetAtTime(value, t, 0.01);
    glide(audio.input.gain, c.on ? 10 ** (c.preamp_db / 20) : 1);
    audio.filters.forEach((f, i) => glide(f.gain, c.on ? c.bands_db[i] : 0));
    const bal = c.balance / 100;
    glide(audio.gl.gain, bal > 0 ? 1 - bal : 1);
    glide(audio.gr.gain, bal < 0 ? 1 + bal : 1);
  };

  const player = () => document.getElementById("movie_player");

  const attempt = (fn) => {
    try {
      return fn();
    } catch (_) {
      return null;
    }
  };

  // ---- Ads -------------------------------------------------------------------------------------

  // While YouTube shows an ad, the player has one of these classes. The Skip button is inside the
  // player, and only shown once YouTube offers it, so this clicks only what YouTube itself offers.
  const adShowing = (p) =>
    !!p && (p.classList.contains("ad-showing") || p.classList.contains("ad-interrupting"));
  const SKIP_SELECTORS = [
    ".ytp-skip-ad-button",
    ".ytp-ad-skip-button-modern",
    ".ytp-ad-skip-button",
    ".ytp-ad-skip-button-slot button",
  ];

  // The most often a skip is posted. A click that misses is tried again after this.
  const SKIP_GAP_MS = 1500;

  // A short description of an element for the log, such as "div.ytp-overlay".
  const describe = (el) => {
    if (!el) return "no element";
    const cls = typeof el.className === "string" ? el.className.trim().split(/\s+/)[0] : "";
    return el.tagName.toLowerCase() + (cls ? "." + cls : "");
  };

  // While an ad shows and T.skipAds is on, posts the centre of YouTube's visible Skip button as
  // {t: "adskip", x, y}. Rust then sends a trusted click there: YouTube pauses the ad for a script
  // click, so no click() here. The button must be hit-testable at its centre (the video view makes
  // the page's elements visibility hidden, which elementFromPoint skips, so VIEW_CSS shows them).
  // If something else covers the button, that is reported once per ad.
  const trySkipAd = () =>
    attempt(() => {
      const p = player();
      if (!adShowing(p)) {
        T.adCovered = false;
        return;
      }
      if (!T.skipAds) return;
      const now = Date.now();
      if (now - T.skipAt < SKIP_GAP_MS) return;
      for (const sel of SKIP_SELECTORS) {
        const btn = p.querySelector(sel);
        if (!btn || getComputedStyle(btn).display === "none") continue;
        const r = btn.getBoundingClientRect();
        if (!(r.width > 0 && r.height > 0)) continue;
        const x = r.left + r.width / 2;
        const y = r.top + r.height / 2;
        const hit = document.elementFromPoint(x, y);
        if (!hit || (hit !== btn && !btn.contains(hit))) {
          if (!T.adCovered) {
            T.adCovered = true;
            post({ t: "error", where: "adskip", msg: `covered by ${describe(hit)}` });
          }
          return;
        }
        T.skipAt = now;
        post({ t: "adskip", x, y });
        return;
      }
    });

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
      ad: !!attempt(() => adShowing(p)),
    };
  };

  const isPlaying = (ps) => ps === 1 || ps === 3;

  // Posts one snapshot now, and the end of a video once (player state 0 after something else).
  // Called by the timer and by the media events below, so the state does not depend on the timer
  // alone. A hidden webview runs its timers slowly, but the media events still arrive.
  const setMediaState = (state) => {
    if (media.state === state) return;
    media.state = state;
    try {
      media.native.playbackState.call(media.ms, state);
    } catch (_) {}
  };

  // Keeps the Now Playing state and position in step with the player (native media only).
  const syncMedia = (msg) => {
    if (!media.native) return;
    if (!media.hasTrack) {
      setMediaState("none");
      return;
    }
    const state = msg.ps === null ? "none" : isPlaying(msg.ps) ? "playing" : "paused";
    setMediaState(state);
    if (state === "none") return;
    const dur = msg.dur;
    if (!(dur > 0) || !Number.isFinite(dur) || !Number.isFinite(msg.cur)) return;
    const position = Math.min(Math.max(msg.cur, 0), dur);
    try {
      media.native.setPositionState.call(media.ms, { duration: dur, playbackRate: 1, position });
    } catch (_) {}
  };

  // TubeAmp's track, as Rust sends it (`nowplaying`). An empty title clears the entry.
  const applyNowPlaying = (c) => {
    if (!media.native) return;
    const title = String(c.title || "").trim();
    media.hasTrack = title !== "";
    let meta = null;
    if (media.hasTrack && typeof MediaMetadata === "function") {
      const src = c.artwork ? String(c.artwork) : "";
      meta = new MediaMetadata({
        title,
        artist: String(c.artist || ""),
        album: String(c.album || ""),
        artwork: src ? [{ src }] : [],
      });
    }
    media.meta = meta;
    try {
      media.native.metadata.call(media.ms, meta);
    } catch (_) {}
    // The state is sent again with the next report, as setting the metadata can reset it.
    media.state = null;
  };

  const report = () => {
    trySkipAd();
    const msg = snapshot(player(), Date.now());
    const ps = msg.ps;
    post(msg);
    syncMedia(msg);
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
    attachAudio(v);
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

  // A MutationObserver on the player, set up once when the player first exists. An ad's class and
  // its Skip button appear as DOM mutations, so the click does not wait for the next tick. The
  // observer does nothing unless skipping is on and an ad is showing.
  const watchAds = () => {
    const p = player();
    if (!p || T.adObserver) return;
    T.adObserver = new MutationObserver(() => {
      attempt(() => {
        if (!T.skipAds || !adShowing(p)) return;
        trySkipAd();
      });
    });
    T.adObserver.observe(p, {
      attributes: true,
      attributeFilter: ["class", "style"],
      childList: true,
      subtree: true,
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
${SKIP_SELECTORS.map((s) => `html.tubeamp-video ${s}, html.tubeamp-video ${s} *`).join(",\n")} { visibility: visible !important; }
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
      watchAds();
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
    if (c.t === "nowplaying") {
      try {
        applyNowPlaying(c);
      } catch (e) {
        post({ t: "error", where: "cmd:nowplaying", msg: String(e) });
      }
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
    if (c.t === "skipads") {
      T.skipAds = !!c.on;
      trySkipAd();
      return;
    }
    if (c.t === "eq") {
      if (!pageAudio) return;
      T.audio.eq = c;
      try {
        applyEq();
      } catch (e) {
        post({ t: "error", where: "cmd:eq", msg: String(e) });
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
