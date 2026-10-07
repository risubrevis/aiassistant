import { openUrl, openPath } from "@tauri-apps/plugin-opener";
import { mediaReadDataUrl } from "$lib/tauri";
import { m } from "$lib/i18n";

export interface MediaImage {
  src: string;
  alt: string;
  orig?: string;
}

export interface EnhanceProseParams {
  onopen: (images: MediaImage[], index: number) => void;
}

const PLAY_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="6 3 20 12 6 21 6 3"/></svg>';
const PAUSE_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="14" y="4" width="4" height="16" rx="1"/><rect x="6" y="4" width="4" height="16" rx="1"/></svg>';
const EXTERNAL_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/></svg>';

function isLocalPath(s: string): boolean {
  if (!s) return false;
  if (s.startsWith("file://")) return true;
  if (s.startsWith("/")) return true;
  return /^[A-Za-z]:[\\/]/.test(s);
}

function toLocalPath(s: string): string {
  if (s.startsWith("file://")) {
    let p = s.slice(7);
    // Windows file URLs look like file:///C:/... — drop the leading slash
    if (/^\/[A-Za-z]:[\\/]/.test(p)) p = p.slice(1);
    try {
      return decodeURIComponent(p);
    } catch {
      return p;
    }
  }
  return s;
}

export async function openExternalLocation(src: string): Promise<void> {
  try {
    if (src.startsWith("http://") || src.startsWith("https://")) {
      await openUrl(src);
    } else if (isLocalPath(src)) {
      await openPath(toLocalPath(src));
    } else {
      await openUrl(src);
    }
  } catch (e) {
    console.error("openExternalLocation failed", e);
  }
}

function replaceWithFallback(el: Element, src: string, kind: string): void {
  const box = document.createElement("div");
  box.className = "md-media-fallback";
  box.dataset.kind = kind;

  const icon = document.createElement("span");
  icon.innerHTML = EXTERNAL_SVG;
  icon.setAttribute("aria-hidden", "true");

  const btn = document.createElement("button");
  btn.type = "button";
  btn.textContent = m.media_load_failed();
  btn.addEventListener("click", () => {
    void openExternalLocation(src);
  });

  box.append(icon, btn);
  el.replaceWith(box);
}

// Themed chip shown when audio data fails to load; opens the original source externally
function fallbackChip(src: string, title: string): HTMLElement {
  const chip = document.createElement("div");
  chip.className = "md-audio-player md-audio-failed";

  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "md-audio-ext";
  btn.setAttribute("aria-label", m.media_open_external());
  btn.setAttribute("title", m.media_open_external());
  btn.innerHTML = EXTERNAL_SVG;
  btn.addEventListener("click", () => {
    void openExternalLocation(src);
  });
  chip.appendChild(btn);

  const label = document.createElement("span");
  label.className = "md-audio-title";
  label.textContent = m.media_load_failed();
  chip.appendChild(label);

  if (title) {
    const titleEl = document.createElement("span");
    titleEl.className = "md-audio-title";
    titleEl.textContent = title;
    chip.appendChild(titleEl);
  }

  return chip;
}

function fmtTime(s: number): string {
  if (!Number.isFinite(s) || s < 0) return "0:00";
  const total = Math.floor(s);
  const h = Math.floor(total / 3600);
  const min = Math.floor((total % 3600) / 60);
  const sec = total % 60;
  const ss = sec.toString().padStart(2, "0");
  if (h > 0) return `${h}:${min.toString().padStart(2, "0")}:${ss}`;
  return `${min}:${ss}`;
}

function buildAudioPlayer(slot: HTMLElement): void {
  const src = slot.getAttribute("data-src") ?? "";
  const title = slot.getAttribute("data-title") ?? "";

  const player = document.createElement("div");
  player.className = "md-audio-player";

  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "md-audio-btn";
  btn.setAttribute("aria-label", m.media_play());
  btn.setAttribute("title", m.media_play());
  btn.innerHTML = PLAY_SVG;

  const seek = document.createElement("input");
  seek.className = "md-audio-seek";
  seek.type = "range";
  seek.min = "0";
  seek.max = "100";
  seek.step = "0.1";
  seek.value = "0";
  seek.setAttribute("aria-label", m.media_seek());

  const timeLabel = document.createElement("span");
  timeLabel.className = "md-audio-time";
  timeLabel.textContent = "0:00 / 0:00";

  const ext = document.createElement("button");
  ext.type = "button";
  ext.className = "md-audio-ext";
  ext.setAttribute("aria-label", m.media_open_external());
  ext.setAttribute("title", m.media_open_external());
  ext.innerHTML = EXTERNAL_SVG;
  ext.addEventListener("click", () => {
    void openExternalLocation(src);
  });

  const audio = document.createElement("audio");
  audio.className = "md-audio-el";
  audio.preload = "metadata";

  player.append(btn, seek, timeLabel, ext);
  if (title) {
    const titleEl = document.createElement("span");
    titleEl.className = "md-audio-title";
    titleEl.textContent = title;
    player.appendChild(titleEl);
  }
  player.appendChild(audio);
  slot.replaceChildren(player);

  let seeking = false;

  const updateTime = () => {
    timeLabel.textContent =
      fmtTime(audio.currentTime) + " / " + fmtTime(audio.duration || 0);
  };

  audio.addEventListener("loadedmetadata", () => {
    seek.max = String(audio.duration || 0);
    updateTime();
  });
  audio.addEventListener("timeupdate", () => {
    if (!seeking) seek.value = String(audio.currentTime);
    updateTime();
  });
  audio.addEventListener("ended", () => {
    btn.innerHTML = PLAY_SVG;
    btn.setAttribute("aria-label", m.media_play());
    seek.value = "0";
    updateTime();
  });
  audio.addEventListener("play", () => {
    btn.innerHTML = PAUSE_SVG;
    btn.setAttribute("aria-label", m.media_pause());
  });
  audio.addEventListener("pause", () => {
    btn.innerHTML = PLAY_SVG;
    btn.setAttribute("aria-label", m.media_play());
  });

  btn.addEventListener("click", () => {
    if (audio.paused) void audio.play().catch(() => {});
    else audio.pause();
  });
  seek.addEventListener("input", () => {
    seeking = true;
    audio.currentTime = parseFloat(seek.value);
  });
  seek.addEventListener("change", () => {
    seeking = false;
  });

  if (isLocalPath(src)) {
    void mediaReadDataUrl(toLocalPath(src))
      .then((url) => {
        audio.src = url;
      })
      .catch((e) => {
        console.error("media audio load failed", e);
        slot.replaceWith(fallbackChip(src, title));
      });
  } else {
    audio.src = src;
  }
}

export function enhanceProse(
  node: HTMLElement,
  params: EnhanceProseParams,
): { update(params: EnhanceProseParams): void; destroy(): void } {
  let onopen = params.onopen;

  const scan = () => {
    const imgs = node.querySelectorAll<HTMLImageElement>("img.md-img:not([data-md-enh])");
    for (const img of imgs) {
      img.setAttribute("data-md-enh", "");
      const originalSrc = img.getAttribute("src") ?? "";
      img.setAttribute("data-orig-src", originalSrc);
      if (isLocalPath(originalSrc)) {
        void mediaReadDataUrl(toLocalPath(originalSrc))
          .then((url) => {
            img.src = url;
          })
          .catch((e) => {
            console.error("media image load failed", e);
            replaceWithFallback(img, originalSrc, "image");
          });
      }
    }

    const videos = node.querySelectorAll<HTMLVideoElement>(
      "video.md-video:not([data-md-enh])",
    );
    for (const video of videos) {
      video.setAttribute("data-md-enh", "");
      const originalSrc = video.getAttribute("src") ?? "";
      video.setAttribute("data-orig-src", originalSrc);

      // Wrap synchronously BEFORE the async src resolution so the DOM move
      // can never race the data-URL update
      const wrapper = document.createElement("div");
      wrapper.className = "md-video-wrap";
      video.replaceWith(wrapper);
      wrapper.appendChild(video);

      const ext = document.createElement("button");
      ext.type = "button";
      ext.className = "md-video-ext";
      ext.setAttribute("aria-label", m.media_open_external());
      ext.setAttribute("title", m.media_open_external());
      ext.innerHTML = EXTERNAL_SVG;
      ext.addEventListener("click", () => {
        void openExternalLocation(originalSrc);
      });
      wrapper.appendChild(ext);

      if (isLocalPath(originalSrc)) {
        void mediaReadDataUrl(toLocalPath(originalSrc))
          .then((url) => {
            video.src = url;
          })
          .catch((e) => {
            console.error("media video load failed", e);
            replaceWithFallback(video, originalSrc, "video");
          });
      }
    }

    const slots = node.querySelectorAll<HTMLElement>(
      "div.md-audio-slot:not([data-md-mounted])",
    );
    for (const slot of slots) {
      slot.setAttribute("data-md-mounted", "");
      buildAudioPlayer(slot);
    }
  };

  const pauseAudioIn = (root: Element) => {
    root.querySelectorAll<HTMLAudioElement>("audio.md-audio-el").forEach((a) => {
      a.pause();
    });
  };

  const observer = new MutationObserver((mutations) => {
    for (const mut of mutations) {
      for (const removed of mut.removedNodes) {
        if (removed instanceof HTMLElement) pauseAudioIn(removed);
      }
    }
    scan();
  });
  observer.observe(node, { childList: true, subtree: true });
  scan();

  const onClick = (e: MouseEvent) => {
    const target = e.target as HTMLElement | null;
    const img = target?.closest<HTMLImageElement>("img.md-img");
    if (img) {
      e.preventDefault();
      e.stopPropagation();
      const all = node.querySelectorAll<HTMLImageElement>("img.md-img");
      const images: MediaImage[] = Array.from(all, (el) => ({
        src: el.getAttribute("src") ?? el.src,
        alt: el.getAttribute("alt") ?? "",
        orig: el.getAttribute("data-orig-src") || undefined,
      }));
      const clickedSrc = img.getAttribute("src") ?? img.src;
      const index = images.findIndex((item) => item.src === clickedSrc);
      if (index >= 0) onopen(images, index);
      return;
    }
    const a = target?.closest<HTMLAnchorElement>("a[href]");
    if (a) {
      const href = a.getAttribute("href");
      if (href && href !== "#") {
        e.preventDefault();
        e.stopPropagation();
        void openExternalLocation(href);
      }
    }
  };
  node.addEventListener("click", onClick);

  return {
    update(next: EnhanceProseParams) {
      onopen = next.onopen;
    },
    destroy() {
      observer.disconnect();
      node.removeEventListener("click", onClick);
      pauseAudioIn(node);
    },
  };
}