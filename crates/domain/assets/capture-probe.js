// Injected after notify-shim.js where the engine has no capture-state API (WebView2). Reports
// whether the page, or an embedded frame (Element Call, Jitsi), holds a live microphone or
// camera track; Rust reads window.__atrium.capturing() with every poll and decides.
(() => {
  try {
    const tracks = new Set();
    const wrap = (owner, name) => {
      try {
        const original = owner && owner[name];
        if (typeof original !== "function") return;
        owner[name] = async function (...args) {
          const stream = await original.apply(this, args);
          try { stream.getTracks().forEach((track) => tracks.add(track)); } catch (e) {}
          return stream;
        };
      } catch (e) {}
    };
    wrap(navigator.mediaDevices, "getUserMedia");
    try {
      const clone = MediaStreamTrack.prototype.clone;
      MediaStreamTrack.prototype.clone = function () {
        const copy = clone.call(this);
        try { if (tracks.has(this)) tracks.add(copy); } catch (e) {}
        return copy;
      };
    } catch (e) {}
    // A track the page disabled stays live: muting oneself is not hanging up.
    const live = () => {
      for (const track of tracks) if (track.readyState === "ended") tracks.delete(track);
      return tracks.size > 0;
    };
    const TAG = "__atriumCapturing";
    if (window === top) {
      const frames = new Map();
      addEventListener("message", (event) => {
        try { if (event.data && event.data[TAG] === true) frames.set(event.source, Date.now()); } catch (e) {}
      });
      const capturing = () => {
        try {
          for (const [frame, at] of frames) if (Date.now() - at > 3000) frames.delete(frame);
          return live() || frames.size > 0;
        } catch (e) { return false; }
      };
      if (window.__atrium) window.__atrium.capturing = capturing;
    } else {
      setInterval(() => {
        try { if (live()) top.postMessage({ [TAG]: true }, "*"); } catch (e) {}
      }, 1000);
    }
  } catch (e) {}
})();
