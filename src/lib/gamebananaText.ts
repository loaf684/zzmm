/**
 * GameBanana's `_sText` field is an HTML fragment (with the occasional BBCode
 * fallback), not plain text. Rendering it verbatim leaks `<br>` and `<font>`
 * tags into the UI, so this turns it into readable, safe segments.
 *
 * GameBanana also rewrites outbound links through `/linkfilter?url=...`, which
 * makes descriptions show long redirect URLs. Links are unwrapped and given a
 * short domain/path label instead.
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

const LINK_MARKER = "\uE000";
const LINK_PATTERN = /\uE000(\d+)\uE000/g;
const URL_PATTERN = /https?:\/\/[^\s<>"']+/gi;

interface ParsedLink {
  text: string;
  href: string;
}

export type GameBananaSegment =
  | { type: "text"; text: string }
  | ({ type: "link" } & ParsedLink);

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

function decodeOnce(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

/** Follow GameBanana's `/linkfilter` redirects and keep only http(s) URLs. */
export function cleanGameBananaUrl(raw: string): string | null {
  let current = raw.trim().replace(/&amp;/gi, "&");
  if (!current) return null;

  for (let depth = 0; depth < 4; depth += 1) {
    if (!/^https?:\/\//i.test(current)) {
      const decoded = decodeOnce(current);
      if (decoded === current) return null;
      current = decoded;
    }

    let url: URL;
    try {
      url = new URL(current);
    } catch {
      return null;
    }

    if (url.protocol !== "http:" && url.protocol !== "https:") return null;

    const isLinkFilter =
      /(^|\.)gamebanana\.com$/i.test(url.hostname) &&
      url.pathname.replace(/\/+$/, "").toLowerCase() === "/linkfilter";

    if (!isLinkFilter) return url.toString();

    const target = url.searchParams.get("url")?.trim();
    if (!target) return null;
    current = /^https?:\/\//i.test(target) ? target : decodeOnce(target);
  }

  return /^https?:\/\//i.test(current) ? current : null;
}

function looksLikeUrl(value: string): boolean {
  return (
    /^(?:https?:\/\/|www\.)/i.test(value) ||
    /^[a-z0-9.-]+\.[a-z]{2,}(?:\/\S*)?$/i.test(value)
  );
}

/** `https://www.patreon.com/creator` -> `patreon.com/creator`. */
function displayUrl(href: string): string {
  try {
    const url = new URL(href);
    const host = url.hostname.replace(/^www\./i, "");
    let path = url.pathname;
    try {
      path = decodeURIComponent(path);
    } catch {
      // Keep the encoded path if it contains malformed escapes.
    }
    path = path.replace(/\/+$/, "");
    const value = `${host}${path}`;
    return value.length > 48 ? host : value;
  } catch {
    return href;
  }
}

function linkLabel(href: string, label: string): string {
  const trimmed = label.replace(/\s+/g, " ").trim();
  if (trimmed && !looksLikeUrl(trimmed)) return trimmed;
  return displayUrl(href);
}

function createLink(parts: string[], links: ParsedLink[], label: string, href: string): void {
  const index = links.length;
  links.push({ text: linkLabel(href, label), href });
  parts.push(`${LINK_MARKER}${index}${LINK_MARKER}`);
}

/** Turn bare URLs in text nodes into links too, without swallowing punctuation. */
function pushTextWithLinks(parts: string[], links: ParsedLink[], value: string): void {
  let cursor = 0;

  for (const match of value.matchAll(URL_PATTERN)) {
    const index = match.index ?? 0;
    const raw = match[0];
    const trailing = raw.match(/[),.;:!?]+$/)?.[0] ?? "";
    const candidate = trailing ? raw.slice(0, -trailing.length) : raw;
    const href = cleanGameBananaUrl(candidate);
    if (!href) continue;

    if (index > cursor) parts.push(value.slice(cursor, index));
    createLink(parts, links, candidate, href);
    if (trailing) parts.push(trailing);
    cursor = index + raw.length;
  }

  if (cursor < value.length) parts.push(value.slice(cursor));
}

function htmlToSegments(html: string): GameBananaSegment[] {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  const parts: string[] = [];
  const links: ParsedLink[] = [];

  const walk = (node: Node) => {
    if (node.nodeType === 3) {
      pushTextWithLinks(parts, links, node.textContent ?? "");
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
      const label = element.textContent ?? "";
      const href = cleanGameBananaUrl(element.getAttribute("href") ?? "");
      if (href) createLink(parts, links, label, href);
      else pushTextWithLinks(parts, links, label);
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

  return normalizeSegments(parts.join(""), links);
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

function normalizeSegments(text: string, links: ParsedLink[]): GameBananaSegment[] {
  const normalized = normalize(text);
  const segments: GameBananaSegment[] = [];
  let cursor = 0;

  for (const match of normalized.matchAll(LINK_PATTERN)) {
    const index = match.index ?? 0;
    if (index > cursor) {
      segments.push({ type: "text", text: normalized.slice(cursor, index) });
    }

    const link = links[Number(match[1])];
    if (link) segments.push({ type: "link", ...link });
    cursor = index + match[0].length;
  }

  if (cursor < normalized.length) {
    segments.push({ type: "text", text: normalized.slice(cursor) });
  }

  return segments;
}

/** Parse GameBanana's rich-text description into safe text/link segments. */
export function parseGameBananaDescription(
  raw: string | null | undefined,
): GameBananaSegment[] {
  if (!raw) return [];

  const source = bbcodeToText(raw);

  if (typeof DOMParser === "undefined") {
    // Browser-only app, but keep a safe fallback for tests or SSR tooling.
    const parts: string[] = [];
    const links: ParsedLink[] = [];
    pushTextWithLinks(parts, links, source);
    return normalizeSegments(parts.join(""), links);
  }

  return htmlToSegments(source);
}
