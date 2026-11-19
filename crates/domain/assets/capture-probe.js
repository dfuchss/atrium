// Injected after notify-shim.js where the engine reports no capture state. Reports whether the
// page, or an embedded frame (Element Call, Jitsi), holds a live microphone or camera track;
// Rust reads window.__atrium.capturing() with every poll and decides.
(() => {
  try {
    const tracks = new Set();
    const getUserMedia = navigator.mediaDevices.getUserMedia;
    navigator.mediaDevices.getUserMedia = async function (...args) {
      const stream = await getUserMedia.apply(this, args);
      try { stream.getTracks().forEach((track) => tracks.add(track)); } catch (e) {}
      return stream;
    };
    const cloneTrack = MediaStreamTrack.prototype.clone;
    MediaStreamTrack.prototype.clone = function () {
      const copy = cloneTrack.call(this);
      if (tracks.has(this)) tracks.add(copy);
      return copy;
    };
    const cloneStream = MediaStream.prototype.clone;
    MediaStream.prototype.clone = function () {
      const copy = cloneStream.call(this);
      try {
        const labels = new Set(this.getTracks().filter((t) => tracks.has(t)).map((t) => t.label));
        copy.getTracks().forEach((t) => labels.has(t.label) && tracks.add(t));
      } catch (e) {}
      return copy;
    };
    // A track the page disabled stays live: muting oneself is not hanging up.
    const live = () => {
      for (const track of tracks) if (track.readyState === "ended") tracks.delete(track);
      return tracks.size > 0;
    };
    const TAG = "__atriumCapturing";
    if (window === top) {
      const frames = new Map();
      addEventListener("message", (event) => {
        if (event.data && event.data[TAG] === true) frames.set(event.source, Date.now());
      });
      if (window.__atrium) window.__atrium.capturing = () => {
        try {
          for (const [frame, at] of frames) if (Date.now() - at > 3000) frames.delete(frame);
          return live() || frames.size > 0;
        } catch (e) { return false; }
      };
    } else {
      setInterval(() => {
        try { if (live()) top.postMessage({ [TAG]: true }, "*"); } catch (e) {}
      }, 1000);
    }
  } catch (e) {}
})();
