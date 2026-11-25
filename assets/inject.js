(() => {
  if (window.__hoprInjected) return;
  window.__hoprInjected = true;

  const post = (payload) => {
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.ipc) {
        window.webkit.messageHandlers.ipc.postMessage(JSON.stringify(payload));
      }
    } catch (_) {}
  };

  // 1. Override Web Notifications -> native macOS notifications via Rust IPC
  class NativeNotification {
    static permission = "granted";
    static maxActions = 0;

    static requestPermission(cb) {
      if (typeof cb === "function") cb("granted");
      return Promise.resolve("granted");
    }

    constructor(title, options = {}) {
      this.title = String(title || "WhatsApp");
      this.body = String(options.body || "");
      this.tag = String(options.tag || "");
      this.onclick = null;
      this.onclose = null;
      this.onerror = null;
      this.onshow = null;

      post({
        type: "notif",
        title: this.title,
        body: this.body,
        tag: this.tag,
      });
    }

    close() {}
    addEventListener() {}
    removeEventListener() {}
    dispatchEvent() { return true; }
  }

  try {
    Object.defineProperty(window, "Notification", {
      value: NativeNotification,
      configurable: true,
      writable: true,
    });
  } catch (_) {
    window.Notification = NativeNotification;
  }

  // 2. Watch document.title for unread badge count, e.g. "(3) WhatsApp"
  let lastBadge = -1;
  const checkTitle = () => {
    const match = /^\((\d+)\)/.exec(document.title || "");
    const n = match ? parseInt(match[1], 10) : 0;
    if (n !== lastBadge) {
      lastBadge = n;
      post({ type: "badge", n });
    }
  };

  const observeTitle = () => {
    const titleEl = document.querySelector("title");
    if (titleEl) {
      const observer = new MutationObserver(checkTitle);
      observer.observe(titleEl, { childList: true, characterData: true, subtree: true });
      checkTitle();
      return true;
    }
    return false;
  };

  if (!observeTitle()) {
    const headObserver = new MutationObserver(() => {
      if (observeTitle()) headObserver.disconnect();
    });
    headObserver.observe(document.documentElement, { childList: true, subtree: true });
  }

  // 3. Report connection state (loading | qr | ready) and sync profile avatar/name
  let lastStatus = "";
  let lastProfileKey = "";

  const tryExtractProfile = () => {
    try {
      let avatarUrl = null;
      let displayName = null;
      let phone = null;

      const headerImg = document.querySelector("#app header img[src]");
      if (headerImg && headerImg.complete && headerImg.naturalWidth > 0) {
        try {
          const canvas = document.createElement("canvas");
          canvas.width = 64;
          canvas.height = 64;
          const ctx = canvas.getContext("2d");
          ctx.drawImage(headerImg, 0, 0, 64, 64);
          avatarUrl = canvas.toDataURL("image/png");
        } catch (_) {
          if (headerImg.src && headerImg.src.startsWith("http")) {
            avatarUrl = headerImg.src;
          }
        }
      }

      const pushname = localStorage.getItem("pushname");
      if (pushname) {
        displayName = pushname.replace(/^"|"$/g, "").trim();
      }
      const wid = localStorage.getItem("last-wid-md") || localStorage.getItem("last-wid");
      if (wid) {
        const num = wid.replace(/^"|"$/g, "").split(":")[0].split("@")[0];
        if (/^\d{7,15}$/.test(num)) {
          phone = "+" + num;
        }
      }

      const key = `${displayName || ""}|${phone || ""}|${avatarUrl ? avatarUrl.slice(0, 48) : ""}`;
      if ((displayName || phone || avatarUrl) && key !== lastProfileKey) {
        lastProfileKey = key;
        post({
          type: "profile",
          display_name: displayName,
          phone,
          avatar_url: avatarUrl,
        });
      }
    } catch (_) {}
  };

  const checkStatus = () => {
    let status = "loading";
    if (document.querySelector("#pane-side, [data-testid='chat-list']")) {
      status = "ready";
      tryExtractProfile();
    } else if (document.querySelector("canvas[aria-label], [data-ref] canvas, [data-testid='qrcode']")) {
      status = "qr";
    }
    if (status !== lastStatus) {
      lastStatus = status;
      post({ type: "status", state: status });
    }
  };

  setInterval(checkStatus, 2500);
  if (document.readyState === "complete") {
    checkStatus();
  } else {
    window.addEventListener("load", checkStatus, { once: true });
  }
})();
