/**
 * GameBanana's `_sText` field is an HTML fragment (with the occasional BBCode
 * fallback), not plain text. Rendering it verbatim leaks `<br>` and `<font>`
 * tags into the UI, so this turns it into readable, safe text.
 */

const BLOCK_TAGS = new Set([
  "address",
  "article",
  "aside",
  "blockquote",
  "div",
  "dl",
  "fieldset",
  "figcaption",
  "figure",
  "footer",
  "form",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "header",
  "hr",
  "main",
  "nav",
  "ol",
  "p",
  "pre",
  "section",
  "table",
  "tr",
  "ul",
]);

/** Convert the BBCode GameBanana occasionally returns before parsing HTML. */
function bbcodeToText(value: string): string {
  return value
    .replace(/\[br\s*\/?\]/gi, "\n")
    .replace(/\[hr\s*\/?\]/gi, "\n\n──────────\n\n")
    .replace(
      /\[url=([^\]]+)\]([\s\S]*?)\[\/url\]/gi,
      (_match, url: string, label: string) =>
        label.trim() && label.trim() !== url ? `${label.trim()} (${url.trim()})` : url.trim(),
    )
    .replace(/\[url\]([\s\S]*?)\[\/url\]/gi, "$1")
    .replace(/\[img[^\]]*\]([\s\S]*?)\[\/img\]/gi, "")
    .replace(/\[img[^\]]*\]/gi, "")
    .replace(/\[\*\]/g, "\n• ")
    .replace(
      /\[\/?(?:b|i|u|s|center|left|right|size|color|font|quote|code|spoiler|list|table|tr|td|h[1-6])(?:=[^\]]*)?\]/gi,
      "",
    );
}

function htmlToText(html: string): string {
  if (typeof DOMParser === "undefined") {
    // Browser-only app, but keep a safe fallback for tests or SSR tooling.
    return html
      .replace(/<br\s*\/?>/gi, "\n")
      .replace(/<[^>]+>/g, "")
      .replace(/&nbsp;/gi, " ")
      .replace(/&amp;/gi, "&")
      .replace(/&lt;/gi, "<")
      .replace(/&gt;/gi, ">");
  }

  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  const parts: string[] = [];

  const walk = (node: Node) => {
    if (node.nodeType === 3) {
      parts.push(node.textContent ?? "");
      return;
    }
    if (node.nodeType !== 1) return;

    const element = node as HTMLElement;
    const tag = element.tagName.toLowerCase();

    if (tag === "br") {
      parts.push("\n");
      return;
    }
    if (tag === "hr") {
      parts.push("\n\n──────────\n\n");
      return;
    }
    if (tag === "img") {
      const alt = element.getAttribute("alt")?.trim();
      if (alt) parts.push(`[${alt}]`);
      return;
    }
    if (tag === "a") {
      const label = (element.textContent ?? "").trim();
      const href = element.getAttribute("href")?.trim() ?? "";
      const external = /^https?:\/\//i.test(href);

      parts.push(label || (external ? href : ""));
      if (external && href && href !== label) {
        parts.push(label ? ` (${href})` : href);
      }
      return;
    }
    if (tag === "li") {
      parts.push("\n• ");
      for (const child of Array.from(element.childNodes)) walk(child);
      parts.push("\n");
      return;
    }

    if (BLOCK_TAGS.has(tag)) parts.push("\n");
    for (const child of Array.from(element.childNodes)) walk(child);
    if (BLOCK_TAGS.has(tag)) parts.push("\n");
  };

  for (const child of Array.from(doc.body.childNodes)) walk(child);

  return parts.join("");
}

function normalize(value: string): string {
  return value
    .replace(/\u00a0/g, " ")
    .replace(/\r\n?/g, "\n")
    .split("\n")
    .map((line) => line.replace(/[ \t]+/g, " ").trim())
    .join("\n")
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .replace(/^\n+|\n+$/g, "");
}

/** Turn GameBanana's rich-text description into readable plain text. */
export function formatGameBananaDescription(raw: string | null | undefined): string {
  if (!raw) return "";
  return normalize(htmlToText(bbcodeToText(raw)));
}
