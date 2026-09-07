import { marked, Renderer } from "marked";
import type { Tokens } from "marked";

marked.setOptions({ breaks: true, gfm: true });

class TableWrapRenderer extends Renderer {
  table(token: Tokens.Table): string {
    return `<div class="table-wrap">\n${super.table(token)}\n</div>`;
  }
}

marked.use({ renderer: new TableWrapRenderer() });

export function renderMarkdown(text: string): string {
  if (!text) return "";
  try {
    return marked.parse(text) as string;
  } catch {
    return text;
  }
}