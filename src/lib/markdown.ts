import { marked, Renderer } from "marked";
import type { Tokens } from "marked";

marked.setOptions({ breaks: true, gfm: true });

const VIDEO_EXT = new Set([
  "mp4",
  "webm",
  "ogv",
  "ogg",
  "mov",
  "m4v",
  "mkv",
  "avi",
]);
const AUDIO_EXT = new Set([
  "mp3",
  "wav",
  "oga",
  "m4a",
  "flac",
  "aac",
  "opus",
  "3gp",
]);

function esc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function hrefExt(href: string): string {
  const path = href.split("?")[0].split("#")[0];
  const dot = path.lastIndexOf(".");
  return dot === -1 ? "" : path.slice(dot + 1).toLowerCase();
}

class MediaRenderer extends Renderer {
  table(token: Tokens.Table): string {
    return `<div class="table-wrap">\n${super.table(token)}\n</div>`;
  }

  image(token: Tokens.Image): string {
    const inner = super
      .image(token)
      .replace("<img ", '<img class="md-img" loading="lazy" ');
    const alt = (token.text || "").trim();
    const title = (token.title || "").trim();
    const caption = alt || title;
    const figcaption = caption
      ? `<figcaption class="md-figcaption">${esc(caption)}</figcaption>`
      : "";
    return `<figure class="md-fig">${inner}${figcaption}</figure>`;
  }

  link(token: Tokens.Link): string {
    const ext = hrefExt(token.href);
    if (VIDEO_EXT.has(ext)) {
      return `<video class="md-video" controls preload="metadata" src="${esc(token.href)}"></video>`;
    }
    if (AUDIO_EXT.has(ext)) {
      const text = (token.text || "").trim();
      const path = token.href.split("?")[0].split("#")[0];
      const filename = path.slice(path.lastIndexOf("/") + 1);
      const audioTitle = text && text !== token.href ? text : filename;
      return `<div class="md-audio-slot" data-src="${esc(token.href)}" data-title="${esc(audioTitle)}"></div>`;
    }
    return super.link(token);
  }
}

marked.use({ renderer: new MediaRenderer() });

export function renderMarkdown(text: string): string {
  if (!text) return "";
  try {
    return marked.parse(text) as string;
  } catch {
    return text;
  }
}