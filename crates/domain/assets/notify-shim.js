// Injected into every service view. Collects web notifications into a queue that Rust drains
// via eval_with_callback; the page gets no Tauri IPC access.
(() => {
  if (window.__msgr) return;
  // Bounded until the next drain (every second): a page cannot pile up notifications.
  const MAX_QUEUED = 50;
  const q = [], live = new Map();
  let seq = 0;
  const push = (title, o, obj) => {
    const id = ++seq;
    if (obj) live.set(id, obj);
    if (q.length >= MAX_QUEUED) live.delete(q.shift().id);
    q.push({ id, title: String(title), body: (o && o.body) || '', tag: (o && o.tag) || null, ts: Date.now() });
  };
  class N extends EventTarget {
    constructor(title, o = {}) { super(); this.title = title; this.body = o.body || ''; this.tag = o.tag || ''; this.onclick = null; push(title, o, this); }
    close() {}
    static get permission() { return 'granted'; }
    static requestPermission(cb) { if (cb) cb('granted'); return Promise.resolve('granted'); }
  }
  // Not writable/configurable: tauri-plugin-notification injects its own window.Notification into
  // every webview (calling IPC that service pages may not use). Whichever script runs later, ours wins.
  Object.defineProperty(window, 'Notification', { value: N, writable: false, configurable: false });
  const SWR = window.ServiceWorkerRegistration;
  if (SWR) SWR.prototype.showNotification = function (t, o) { push(t, o, null); return Promise.resolve(); };
  window.__msgr = {
    q, probe: null,
    drain() { return q.splice(0, q.length); },
    click(id) { const n = live.get(id); if (n) { window.focus(); if (n.onclick) n.onclick(new Event('click')); n.dispatchEvent(new Event('click')); } },
  };
  // Embedded WKWebViews silently drop clicks on <a target="_blank"> (no navigation, no new-window
  // request), while window.open() reaches the app's new-window handler. Re-route such clicks the
  // page did not handle itself; the app then applies its link policy (system browser / app window).
  document.addEventListener('click', (e) => {
    if (e.defaultPrevented || e.button !== 0) return;
    const a = e.target instanceof Element ? e.target.closest('a[href]') : null;
    if (!a || !/^https?:/i.test(a.href)) return;
    const newWindow = (a.target && a.target !== '_self' && a.target !== '_top' && a.target !== '_parent') || e.metaKey;
    if (!newWindow) return;
    e.preventDefault();
    window.open(a.href, '_blank', 'noopener');
  });
})();
