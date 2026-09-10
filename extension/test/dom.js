// A tiny DOM for the fixture HTML in this directory.
//
// The companion ships no runtime dependency and this test suite adds none, so
// the fixtures are parsed here. It handles exactly what the fixtures use:
// well-formed elements, quoted attributes, text, comments, and void tags. It
// is a test helper, never shipped, and it is deliberately small enough to
// read in one sitting.

const VOID = new Set([
  "area", "base", "br", "col", "embed", "hr", "img", "input",
  "link", "meta", "param", "source", "track", "wbr",
]);

// Elements whose text is markup, not prose; the extractor skips them anyway,
// but the parser must not treat their contents as tags.
const RAW_TEXT = new Set(["script", "style"]);

class Element {
  constructor(tagName, attributes) {
    this.tagName = tagName.toUpperCase();
    this.attributes = attributes;
    this.children = [];
    this.text = "";
  }

  getAttribute(name) {
    const value = this.attributes[name.toLowerCase()];
    return value === undefined ? null : value;
  }

  get value() {
    // A `<textarea>` carries its value as its text.
    return this.tagName === "TEXTAREA" ? this.textContent : undefined;
  }

  get textContent() {
    let out = this.text;
    for (const child of this.children) {
      out += child.textContent;
    }
    return out;
  }

  get classList() {
    return String(this.getAttribute("class") || "").split(/\s+/).filter(Boolean);
  }

  querySelectorAll(selector) {
    const matchers = selector.split(",").map((part) => compile(part.trim()));
    const out = [];
    for (const matcher of matchers) {
      collect(this, matcher, out);
    }
    return out;
  }
}

/** A descendant chain of simple selectors, applied right to left. */
function compile(selector) {
  const steps = selector.split(/\s+/).filter(Boolean).map(simple);
  return (element, ancestors) => {
    if (!steps[steps.length - 1](element)) {
      return false;
    }
    let remaining = steps.slice(0, -1);
    for (let i = ancestors.length - 1; i >= 0 && remaining.length > 0; i -= 1) {
      if (remaining[remaining.length - 1](ancestors[i])) {
        remaining = remaining.slice(0, -1);
      }
    }
    return remaining.length === 0;
  };
}

/** `tag`, `.class`, `#id`, `[attr]`, `[attr='value']`, and combinations. */
function simple(part) {
  const tests = [];
  const pattern = /(^[a-zA-Z][\w-]*)|(\.[\w-]+)|(#[\w-]+)|(\[[^\]]+\])/g;
  let match;
  while ((match = pattern.exec(part)) !== null) {
    const token = match[0];
    if (token.startsWith(".")) {
      const wanted = token.slice(1);
      tests.push((el) => el.classList.includes(wanted));
    } else if (token.startsWith("#")) {
      const wanted = token.slice(1);
      tests.push((el) => el.getAttribute("id") === wanted);
    } else if (token.startsWith("[")) {
      const body = token.slice(1, -1);
      const eq = body.indexOf("=");
      if (eq < 0) {
        tests.push((el) => el.getAttribute(body.trim()) !== null);
      } else {
        const name = body.slice(0, eq).trim();
        const wanted = body.slice(eq + 1).trim().replace(/^['"]|['"]$/g, "");
        tests.push((el) => el.getAttribute(name) === wanted);
      }
    } else {
      const wanted = token.toUpperCase();
      tests.push((el) => el.tagName === wanted);
    }
  }
  return (el) => tests.every((test) => test(el));
}

function collect(root, matcher, out, ancestors = []) {
  for (const child of root.children) {
    if (matcher(child, ancestors) && !out.includes(child)) {
      out.push(child);
    }
    collect(child, matcher, out, [...ancestors, child]);
  }
}

/** Parse fixture HTML into a root element with `querySelectorAll`. */
export function parse(html) {
  const root = new Element("#document", {});
  const stack = [root];
  let index = 0;
  while (index < html.length) {
    const open = html.indexOf("<", index);
    if (open < 0) {
      addText(stack, html.slice(index));
      break;
    }
    addText(stack, html.slice(index, open));
    if (html.startsWith("<!--", open)) {
      const close = html.indexOf("-->", open);
      index = close < 0 ? html.length : close + 3;
      continue;
    }
    if (html.startsWith("<!", open)) {
      const close = html.indexOf(">", open);
      index = close < 0 ? html.length : close + 1;
      continue;
    }
    const close = html.indexOf(">", open);
    if (close < 0) {
      break;
    }
    const raw = html.slice(open + 1, close).trim();
    index = close + 1;
    if (raw.startsWith("/")) {
      const name = raw.slice(1).trim().toUpperCase();
      while (stack.length > 1) {
        const popped = stack.pop();
        if (popped.tagName === name) {
          break;
        }
      }
      continue;
    }
    const selfClosing = raw.endsWith("/");
    const body = selfClosing ? raw.slice(0, -1) : raw;
    const space = body.search(/\s/);
    const tag = (space < 0 ? body : body.slice(0, space)).toLowerCase();
    const element = new Element(tag, attributesOf(space < 0 ? "" : body.slice(space)));
    stack[stack.length - 1].children.push(element);
    if (RAW_TEXT.has(tag)) {
      const end = html.toLowerCase().indexOf(`</${tag}>`, index);
      element.text = end < 0 ? html.slice(index) : html.slice(index, end);
      index = end < 0 ? html.length : end + tag.length + 3;
      continue;
    }
    if (!selfClosing && !VOID.has(tag)) {
      stack.push(element);
    }
  }
  return root;
}

function addText(stack, text) {
  if (text.length > 0) {
    stack[stack.length - 1].text += decode(text);
  }
}

function decode(text) {
  return text
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, "&");
}

function attributesOf(raw) {
  const out = {};
  const pattern = /([\w:-]+)(?:\s*=\s*("([^"]*)"|'([^']*)'|([^\s"'>]+)))?/g;
  let match;
  while ((match = pattern.exec(raw)) !== null) {
    const name = match[1].toLowerCase();
    out[name] = match[3] ?? match[4] ?? match[5] ?? "";
  }
  return out;
}
