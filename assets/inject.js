(() => {
  if (window.__hoprInjected) return;
  window.__hoprInjected = true;

  const post = (payload) => {
    const raw = JSON.stringify(payload);
    try {
      if (window.ipc && window.ipc.postMessage) {
        window.ipc.postMessage(raw);
      } else if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.ipc) {
        window.webkit.messageHandlers.ipc.postMessage(raw);
      }
    } catch (_) {}
  };

  // Inject subtle native macOS CSS polish into WhatsApp Web
  const injectNativeCss = () => {
    if (document.getElementById("hopr-native-css")) return;
    const style = document.createElement("style");
    style.id = "hopr-native-css";
    style.textContent = `
      html, body, #app {
        font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif !important;
        -webkit-font-smoothing: antialiased;
      }
      ::-webkit-scrollbar {
        width: 6px;
        height: 6px;
      }
      ::-webkit-scrollbar-thumb {
        background: rgba(128, 128, 128, 0.35);
        border-radius: 3px;
      }
    `;
    (document.head || document.documentElement).appendChild(style);
  };
  if (document.head || document.documentElement) {
    injectNativeCss();
  } else {
    document.addEventListener("DOMContentLoaded", injectNativeCss, { once: true });
  }

  // 1. Override Web Notifications -> native macOS notifications via Rust IPC
  class NativeNotification {
    static permission = "granted";
    static maxActions = 0;

    static requestPermission(cb) {
      if (typeof cb === "function") cb("granted");
      return Promise.resolve("granted");
    }

    constructor(title, options = {}) {
      this.title = String(title || "WhatsApp").slice(0, 200);
      this.body = String(options.body || "").slice(0, 500);
      this.tag = String(options.tag || "").slice(0, 100);
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

  // 3. Report network reachability events (no polling)
  window.addEventListener("online", () => post({ type: "network", online: true }));
  window.addEventListener("offline", () => post({ type: "network", online: false }));
  if (!navigator.onLine) {
    post({ type: "network", online: false });
  }

  // 4. Report connection state (loading | qr | ready) and sync user's own profile avatar/name
  let lastStatus = "";
  let lastProfileKey = "";

  const findOwnProfileImg = () => {
    const selectors = [
      "[data-testid='menu-bar-user-avatar'] img[src]",
      "header button[aria-label*='Profile' i] img[src]",
      "header [role='button'][aria-label*='Profile' i] img[src]",
      "nav button[aria-label*='Profile' i] img[src]",
      "header img[src*='pps.whatsapp.net']",
      "nav img[src*='pps.whatsapp.net']",
    ];
    for (const sel of selectors) {
      const img = document.querySelector(sel);
      if (img && img.src && !img.src.includes("meta") && !img.src.includes("ai")) {
        return img;
      }
    }
    return null;
  };

  const tryExtractProfile = () => {
    try {
      let avatarUrl = null;
      let displayName = null;
      let phone = null;

      const profileImg = findOwnProfileImg();
      if (profileImg && profileImg.complete && profileImg.naturalWidth > 0) {
        try {
          const canvas = document.createElement("canvas");
          canvas.width = 64;
          canvas.height = 64;
          const ctx = canvas.getContext("2d");
          ctx.drawImage(profileImg, 0, 0, 64, 64);
          avatarUrl = canvas.toDataURL("image/jpeg", 0.85);
        } catch (_) {
          if (profileImg.src && profileImg.src.startsWith("https://")) {
            avatarUrl = profileImg.src;
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

  setInterval(checkStatus, 3000);
  if (document.readyState === "complete") {
    checkStatus();
  } else {
    window.addEventListener("load", checkStatus, { once: true });
  }
})();
