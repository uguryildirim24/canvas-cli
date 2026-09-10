// The note renderer: Markdown source in, an inert node tree out.
//
// A note is written by an agent, and an agent reads web pages. The note is
// therefore treated as hostile text from end to end:
//
// - nothing here ever produces HTML. The output is a tree of plain objects,
//   and the panel turns each one into an element with `textContent`. No
//   string this file returns ever becomes markup;
// - `<script>`, `<img>`, and every other tag stays literal text. There is no
//   HTML parser in this file to be confused;
// - a link survives only when it points at the granted origin over `https`
//   with no credentials in it. `javascript:`, `data:`, another host, and a
//   URL carrying a username are shown as text and marked blocked, so the
//   person sees that a link was removed rather than a link that lies;
// - `canvas://` is a reference to a local record. It is shown as a reference,
//   never as something to click, because the browser cannot open it.
//
// The subset is deliberately small: headings, paragraphs, lists, block
// quotes, code, bold, italic, inline code, and links. Anything else is text.
globalThis.canvasCli = globalThis.canvasCli || {};

// A note longer than this was refused by the host before it reached here.
// The renderer repeats the bound so a panel that is fed by anything else
// still cannot be made to lay out a megabyte of text.
const MAX_NOTE_BYTES = 8 * 1024;

// How deep an emphasis run may nest before the rest is taken as text.
const MAX_INLINE_DEPTH = 4;

// How deep a block quote may nest before its body is taken as plain lines.
//
// Each `>` on a line is one level, and each level is one recursive call, so
// an unbounded nesting is a stack overflow — which in this panel is not a
// wrong render but no render at all, approvals included. A note is hostile
// text; `>` repeated eight thousand times fits inside the 8 KiB bound.
const MAX_QUOTE_DEPTH = 6;

/**
 * What may be done with a link target.
 *
 * `allow` is a link the panel opens; `ref` is a `canvas://` reference shown
 * as text; `block` is everything else.
 *
 * @returns {"allow"|"ref"|"block"}
 */
globalThis.canvasCli.linkPolicy = function linkPolicy(href, origin) {
  const raw = String(href || "").trim();
  if (raw.startsWith("canvas://")) {
    return raw.length > "canvas://".length ? "ref" : "block";
  }
  let url;
  try {
    url = new URL(raw);
  } catch {
    return "block";
  }
  if (url.protocol !== "https:") {
    return "block";
  }
  // Credentials in a displayed URL are a way to make the host look like
  // something it is not, and `origin` ignores them.
  if (url.username !== "" || url.password !== "") {
    return "block";
  }
  return url.origin === String(origin || "") ? "allow" : "block";
};

/**
 * Render one note's Markdown source into blocks.
 *
 * @param {string} source the note text, exactly as the host holds it
 * @param {string} origin the granted origin; the only host a link may name
 * @param {number} depth how many block quotes this call is already inside
 * @returns {Array<object>} block nodes: heading, paragraph, list, quote, code
 */
globalThis.canvasCli.renderMarkdown = function renderMarkdown(source, origin, depth = 0) {
  const text = String(source == null ? "" : source);
  const bounded = globalThis.canvasCli.truncateUtf8
    ? globalThis.canvasCli.truncateUtf8(text, MAX_NOTE_BYTES).text
    : text;
  const lines = bounded.replace(/\r\n?/g, "\n").split("\n");
  const blocks = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];
    if (line.trim() === "") {
      index += 1;
      continue;
    }
    const fence = /^\s{0,3}(```+|~~~+)\s*(\S*)\s*$/.exec(line);
    if (fence) {
      const [, marker] = fence;
      const body = [];
      index += 1;
      while (index < lines.length && !isClosingFence(lines[index], marker)) {
        body.push(lines[index]);
        index += 1;
      }
      // An unterminated fence ends at the end of the note, not in an error.
      index += 1;
      blocks.push({ type: "code", text: body.join("\n") });
      continue;
    }
    const heading = /^\s{0,3}(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      blocks.push({
        type: "heading",
        level: Math.min(heading[1].length, 3),
        children: inline(heading[2], origin),
      });
      index += 1;
      continue;
    }
    if (/^\s{0,3}>/.test(line)) {
      const body = [];
      while (index < lines.length && /^\s{0,3}>/.test(lines[index])) {
        body.push(lines[index].replace(/^\s{0,3}>\s?/, ""));
        index += 1;
      }
      const inner = body.join("\n");
      // Past the bound the quote's body is text, exactly as an emphasis run
      // past `MAX_INLINE_DEPTH` is text. Nothing is dropped and nothing
      // recurses further.
      blocks.push({
        type: "quote",
        children:
          depth < MAX_QUOTE_DEPTH
            ? globalThis.canvasCli.renderMarkdown(inner, origin, depth + 1)
            : [{ type: "paragraph", children: [{ type: "text", text: inner }] }],
      });
      continue;
    }
    const bullet = /^\s{0,3}([-*+]|\d{1,9}[.)])\s+(.*)$/.exec(line);
    if (bullet) {
      const ordered = !/^[-*+]$/.test(bullet[1]);
      const items = [];
      while (index < lines.length) {
        const item = /^\s{0,3}([-*+]|\d{1,9}[.)])\s+(.*)$/.exec(lines[index]);
        if (item === null) {
          break;
        }
        // A bullet after a number starts a new list, and the other way round.
        if (!/^[-*+]$/.test(item[1]) !== ordered) {
          break;
        }
        items.push(inline(item[2], origin));
        index += 1;
      }
      blocks.push({ type: "list", ordered, items });
      continue;
    }
    const paragraph = [];
    while (
      index < lines.length &&
      lines[index].trim() !== "" &&
      !/^\s{0,3}(#{1,6}\s|>|```|~~~)/.test(lines[index]) &&
      !/^\s{0,3}([-*+]|\d{1,9}[.)])\s/.test(lines[index])
    ) {
      paragraph.push(lines[index].trim());
      index += 1;
    }
    blocks.push({ type: "paragraph", children: inline(paragraph.join(" "), origin) });
  }
  return blocks;
};

/** Whether a line closes a fence opened with `marker`. */
function isClosingFence(line, marker) {
  const closing = /^\s{0,3}(```+|~~~+)\s*$/.exec(line);
  return Boolean(closing) && closing[1][0] === marker[0] && closing[1].length >= marker.length;
}

/**
 * The inline pass: code spans, links, emphasis, and text.
 *
 * Everything that is not one of those is text, including anything that looks
 * like a tag. That is the whole defence, and it holds because no branch of
 * this function ever emits markup.
 */
function inline(source, origin, depth = 0) {
  const text = String(source || "");
  const out = [];
  let plain = "";
  let i = 0;

  const flush = () => {
    if (plain !== "") {
      out.push({ type: "text", text: plain });
      plain = "";
    }
  };

  while (i < text.length) {
    const rest = text.slice(i);

    const code = /^`+/.exec(rest);
    if (code) {
      const ticks = code[0];
      const end = rest.indexOf(ticks, ticks.length);
      if (end !== -1) {
        flush();
        out.push({ type: "code", text: rest.slice(ticks.length, end) });
        i += end + ticks.length;
        continue;
      }
    }

    // `![alt](url)`: an image is never fetched and never shown. Its alt text
    // is prose the person may want, so it stays, and nothing else does.
    const image = /^!\[([^\]]*)\]\(([^)\s]*)[^)]*\)/.exec(rest);
    if (image) {
      flush();
      out.push({ type: "text", text: image[1] });
      i += image[0].length;
      continue;
    }

    const link = /^\[([^\]]*)\]\(([^)\s]*)[^)]*\)/.exec(rest);
    if (link) {
      flush();
      out.push(linkNode(link[1], link[2], origin, depth));
      i += link[0].length;
      continue;
    }

    if (depth < MAX_INLINE_DEPTH) {
      const strong = /^(\*\*|__)(?=\S)([\s\S]*?\S)\1/.exec(rest);
      if (strong) {
        flush();
        out.push({ type: "strong", children: inline(strong[2], origin, depth + 1) });
        i += strong[0].length;
        continue;
      }
      const em = /^([*_])(?=\S)([\s\S]*?\S)\1/.exec(rest);
      if (em) {
        flush();
        out.push({ type: "em", children: inline(em[2], origin, depth + 1) });
        i += em[0].length;
        continue;
      }
    }

    if (rest[0] === "\\" && rest.length > 1) {
      plain += rest[1];
      i += 2;
      continue;
    }

    plain += rest[0];
    i += 1;
  }
  flush();
  return out;
}

/** One link, or the text it would have been. */
function linkNode(label, href, origin, depth) {
  const shown = label === "" ? href : label;
  const children = inline(shown, origin, depth + 1);
  switch (globalThis.canvasCli.linkPolicy(href, origin)) {
    case "allow":
      return { type: "link", href: String(href).trim(), children };
    case "ref":
      return { type: "ref", ref: String(href).trim(), children };
    default:
      // The label alone would hide that a link was there at all, and the
      // person is the one deciding whether to trust this note.
      return { type: "blocked", href: String(href).trim(), children };
  }
}

globalThis.canvasCli.MAX_NOTE_BYTES = MAX_NOTE_BYTES;
globalThis.canvasCli.MAX_QUOTE_DEPTH = MAX_QUOTE_DEPTH;
