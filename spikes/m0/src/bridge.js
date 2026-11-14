// M0 spike bridge: injected into music.youtube.com.
// Proves (1) player state + metadata are readable, (2) Web Audio graph on the
// MSE-fed <video> is audible and EQ changes the sound, (3) control from Rust.
(() => {
  if (window.top !== window || window.__tubeamp) return;
  const post = (o) => { try { window.ipc.postMessage(JSON.stringify(o)); } catch (_) {} };
  const FREQS = [60, 170, 310, 600, 1000, 3000, 6000, 12000, 14000, 16000];
  const T = (window.__tubeamp = { ctx: null, filters: [], pre: null, pan: null, analyser: null, video: null, err: null });

  function attach(v) {
    if (T.video === v) return;
    if (window.__M0_NOGRAPH) { T.video = v; post({ t: "nograph" }); return; }
    try {
      const ctx = T.ctx || (T.ctx = new (window.AudioContext || window.webkitAudioContext)());
      const src = ctx.createMediaElementSource(v);
      T.pre = ctx.createGain();
      src.connect(T.pre);
      let node = T.pre;
      T.filters = FREQS.map((f) => {
        const b = ctx.createBiquadFilter();
        b.type = "peaking"; b.frequency.value = f; b.Q.value = 1.4; b.gain.value = 0;
        node.connect(b); node = b; return b;
      });
      T.pan = ctx.createStereoPanner();
      node.connect(T.pan);
      T.analyser = ctx.createAnalyser();
      T.analyser.fftSize = 1024;
      T.pan.connect(T.analyser);
      T.analyser.connect(ctx.destination);
      T.video = v;
      v.addEventListener("play", () => ctx.resume());
      v.addEventListener("ended", () => post({ t: "ended" }));
      post({ t: "attached", sr: ctx.sampleRate });
    } catch (e) {
      T.err = String(e);
      post({ t: "error", where: "attach", msg: T.err });
    }
  }

  function rms() {
    if (!T.analyser) return null;
    const a = new Float32Array(T.analyser.fftSize);
    T.analyser.getFloatTimeDomainData(a);
    let s = 0; for (const x of a) s += x * x;
    return Math.sqrt(s / a.length);
  }

  function player() { return document.getElementById("movie_player"); }

  T.cmd = (c) => {
    const p = player();
    try {
      switch (c.t) {
        case "toggle": p && (p.getPlayerState() === 1 ? p.pauseVideo() : p.playVideo()); break;
        case "next": p && p.nextVideo(); break;
        case "eq": T.filters.forEach((f, i) => (f.gain.value = c.gains[i] ?? 0)); break;
        case "load": p && p.loadVideoById(c.id); break;
      }
      post({ t: "ack", cmd: c.t });
    } catch (e) { post({ t: "error", where: "cmd:" + c.t, msg: String(e) }); }
  };

  setInterval(() => {
    const v = document.querySelector("video");
    if (v) attach(v);
    const p = player();
    let d = null;
    try { d = p && p.getVideoData ? p.getVideoData() : null; } catch (_) {}
    post({
      t: "state",
      id: d && d.video_id, title: d && d.title, author: d && d.author,
      cur: p && p.getCurrentTime ? p.getCurrentTime() : null,
      dur: p && p.getDuration ? p.getDuration() : null,
      ps: p && p.getPlayerState ? p.getPlayerState() : null,
      ctx: T.ctx && T.ctx.state, rms: rms(), hidden: document.hidden,
      src: T.video && String(T.video.currentSrc).slice(0, 12), muted: T.video && T.video.muted, vol: T.video && T.video.volume,
      mse: typeof MediaSource, mms: typeof window.ManagedMediaSource, ct: T.ctx && +T.ctx.currentTime.toFixed(1),
      fmax: T.analyser ? (() => { const b = new Uint8Array(T.analyser.frequencyBinCount); T.analyser.getByteFrequencyData(b); return Math.max(...b); })() : null,
      bands: T.analyser ? (() => { const b = new Float32Array(T.analyser.frequencyBinCount); T.analyser.getFloatFrequencyData(b); const hz = T.ctx.sampleRate / T.analyser.fftSize; const avg = (lo, hi) => { let s = 0, n = 0; for (let i = Math.floor(lo / hz); i <= Math.ceil(hi / hz); i++) { s += b[i]; n++; } return +(s / n).toFixed(1); }; return { low: avg(40, 250), mid: avg(800, 3500), high: avg(6000, 14000) }; })() : null,
      fmt: (() => { try { const st = p.getStatsForNerds && p.getStatsForNerds(); return st ? (st.codecs || "") + " | " + (st.resolution || "") : null; } catch (_) { return null; } })(),
      ms: navigator.mediaSession && navigator.mediaSession.metadata ? navigator.mediaSession.metadata.title : null,
    });
  }, 1000);
})();
