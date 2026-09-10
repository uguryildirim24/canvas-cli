// The side panel: an extension-owned surface (REPORT §3.4).
//
// It is not a Canvas overlay and it is not a chat window. It has no model
// behind it, it opens no database, and it makes no request of its own — not
// to Canvas, not anywhere. Everything it draws arrives from the native host
// through the service worker, and the one thing it sends back is the
// person's decision on a plan.
//
// The rendering rule is absolute: every value from the host reaches the
// document through `textContent`, and every element comes from
// `createElement`. No element in this file is ever built from a string, and
// `tests/companion.rs` checks that none of the ways to do so appears here.

const view = globalThis.canvasCli;

/** @type {object|null} The last state the host sent. */
let current = null;

const root = document.getElementById("panel");

chrome.runtime.onMessage.addListener((message) => {
  if (!message || typeof message.type !== "string") {
    return false;
  }
  if (message.type === "panel") {
    current = message.state;
    draw();
  } else if (message.type === "note" && current) {
    // The host pushes the whole state right after a note; showing the note at
    // once means the person does not wait for the round trip.
    const notes = Array.isArray(current.notes) ? current.notes : [];
    if (!notes.some((note) => note.note_id === message.note.note_id)) {
      current = { ...current, notes: [...notes, message.note] };
      draw();
    }
  }
  return false;
});

chrome.runtime.sendMessage({ type: "panel_hello" }).catch(() => {});

/** Redraw everything. The panel is small; there is nothing to reconcile. */
function draw() {
  root.replaceChildren();
  if (current === null) {
    root.append(el("p", "class", "quiet", text("waiting for canvas-cli")));
    return;
  }
  const model = view.panelView(current);
  root.append(header(model.attachment));
  if (model.attachment.refresh_required) {
    root.append(
      section("Out of date", el("p", "class", "warn", text("refresh: this panel fell behind the log")))
    );
  }
  if (model.approvals.length > 0) {
    root.append(section("Waiting for you", ...model.approvals.map(approval)));
  }
  if (model.follow !== null) {
    root.append(section("Navigation", follow(model.follow)));
  }
  if (model.notes.length > 0) {
    root.append(section("Notes", ...model.notes.map(note)));
  }
  root.append(section("Submissions", ...journals(model.journals)));
}

function header(attachment) {
  const box = el("header");
  box.append(el("h1", null, null, text(attachment.origin || "canvas-cli")));
  box.append(el("p", "class", `state ${attachment.state}`, text(attachment.state)));
  box.append(el("p", "class", "quiet", text(attachment.says)));
  if (attachment.title !== null) {
    box.append(el("p", "class", "page", text(attachment.title)));
  }
  if (attachment.zone !== null) {
    box.append(el("p", "class", "quiet", text(`zone ${attachment.zone}`)));
  }
  if (attachment.consumers.length > 0) {
    box.append(el("p", "class", "quiet", text(`shared with ${attachment.consumers.join(", ")}`)));
  }
  if (attachment.observed_at !== null) {
    box.append(el("p", "class", "quiet", text(`seen ${attachment.observed_at}`)));
  }
  return box;
}

/** One plan, with everything a person needs to recognize the work. */
function approval(plan) {
  const box = el("article", "class", "plan");
  box.append(el("h3", null, null, text(plan.assignment_name || `assignment ${plan.assignment_id}`)));
  box.append(el("p", "class", "quiet", text(`${plan.kind} · course ${plan.course_id}`)));
  if (plan.consumer) {
    box.append(el("p", "class", "quiet", text(`asked for by ${plan.consumer}`)));
  }
  for (const file of plan.files || []) {
    box.append(el("p", "class", "file", text(`${file.name} · ${file.bytes} bytes · ${file.sha256}`)));
  }
  if (plan.text_preview) {
    box.append(el("pre", "class", "preview", text(plan.text_preview)));
  }
  if (plan.url) {
    box.append(el("p", "class", "file", text(plan.url)));
  }
  if (plan.comment) {
    box.append(el("p", "class", "quiet", text(`comment: ${plan.comment}`)));
  }
  box.append(el("p", "class", "digest", text(`plan ${plan.plan_sha256}`)));
  if (plan.input_sha256) {
    box.append(el("p", "class", "digest", text(`input ${plan.input_sha256}`)));
  }
  if (plan.sent_sha256) {
    box.append(el("p", "class", "digest", text(`sent ${plan.sent_sha256}`)));
  }
  box.append(
    el("p", "class", "quiet", text(`attempt ${plan.baseline_attempt} is the baseline · expires ${plan.expires_at}`))
  );
  const actions = el("p", "class", "actions");
  for (const decision of ["approve", "decline", "cancel"]) {
    const button = el("button", "type", "button", text(decision));
    button.addEventListener("click", () => decide(plan, decision));
    actions.append(button);
  }
  box.append(actions);
  return box;
}

/**
 * Send the decision the person made.
 *
 * The handle travels back exactly as the host issued it. The host checks it,
 * the digest, the identity generation, and the consumer again before
 * anything moves; nothing here is trusted, and nothing here decides.
 */
function decide(plan, decision) {
  chrome.runtime
    .sendMessage({
      type: "decision",
      plan_id: plan.plan_id,
      handle: plan.handle,
      plan_sha256: plan.plan_sha256,
      decision,
    })
    .catch(() => {});
}

function follow(status) {
  const box = el("article", "class", "follow");
  box.append(el("p", "class", "page", text(status.url)));
  box.append(el("p", "class", "quiet", text(status.says)));
  if (status.dispatch_ms !== null) {
    box.append(el("p", "class", "quiet", text(`the browser accepted it in ${status.dispatch_ms} ms`)));
  }
  box.append(el("p", "class", "warn", text(status.side_effects)));
  return box;
}

function note(entry) {
  const box = el("article", "class", "note");
  box.append(el("p", "class", "quiet", text(`${entry.consumer || "canvas-cli"} · ${entry.at}`)));
  const body = el("div", "class", "markdown");
  for (const block of entry.blocks) {
    body.append(renderBlock(block));
  }
  box.append(body);
  for (const ref of entry.source_refs) {
    box.append(sourceRef(ref));
  }
  return box;
}

function sourceRef(ref) {
  if (ref.policy === "allow") {
    const line = el("p", "class", "ref");
    line.append(link(ref.ref, ref.ref));
    return line;
  }
  return el("p", "class", "ref", text(ref.ref));
}

function journals(rows) {
  if (rows.length === 0) {
    return [el("p", "class", "quiet", text("no submissions for this page"))];
  }
  return rows.map((row) => {
    const box = el("article", "class", `journal ${row.done ? "done" : "open"}`);
    box.append(el("h3", null, null, text(row.assignment)));
    // The §12.2 name, exactly as the journal holds it.
    box.append(el("p", "class", "state", text(row.state)));
    box.append(el("p", "class", "quiet", text(row.says)));
    for (const line of row.notes) {
      box.append(el("p", "class", "quiet", text(line)));
    }
    box.append(
      el(
        "p",
        "class",
        "quiet",
        text(row.receipt === null ? "no receipt" : `receipt ${row.receipt}`)
      )
    );
    box.append(el("p", "class", "quiet", text(row.updated_at)));
    return box;
  });
}

/** One Markdown block from the renderer, as elements. */
function renderBlock(block) {
  switch (block.type) {
    case "heading":
      return el(`h${block.level + 3}`, null, null, ...block.children.map(renderInline));
    case "list": {
      const list = el(block.ordered ? "ol" : "ul");
      for (const item of block.items) {
        list.append(el("li", null, null, ...item.map(renderInline)));
      }
      return list;
    }
    case "quote": {
      const quote = el("blockquote");
      for (const child of block.children) {
        quote.append(renderBlock(child));
      }
      return quote;
    }
    case "code":
      return el("pre", null, null, text(block.text));
    default:
      return el("p", null, null, ...block.children.map(renderInline));
  }
}

/** One inline node. Text is text; a link is only ever a checked link. */
function renderInline(node) {
  switch (node.type) {
    case "strong":
      return el("strong", null, null, ...node.children.map(renderInline));
    case "em":
      return el("em", null, null, ...node.children.map(renderInline));
    case "code":
      return el("code", null, null, text(node.text));
    case "link":
      return link(node.href, node.children.map(renderInline));
    case "ref":
      return el("span", "class", "ref", ...node.children.map(renderInline));
    case "blocked":
      // The person sees that something claimed to be a link, and sees that
      // it is not one here.
      return el("span", "class", "blocked", ...node.children.map(renderInline), text(" (link removed)"));
    default:
      return text(node.text);
  }
}

/** An anchor to the granted origin, and nothing else can reach this. */
function link(href, body) {
  const anchor = el("a");
  anchor.setAttribute("href", href);
  anchor.setAttribute("rel", "noreferrer noopener");
  anchor.setAttribute("target", "_blank");
  anchor.append(...(Array.isArray(body) ? body : [text(body)]));
  return anchor;
}

function el(tag, attribute, value, ...children) {
  const node = document.createElement(tag);
  if (attribute !== undefined && attribute !== null) {
    node.setAttribute(attribute, value);
  }
  node.append(...children);
  return node;
}

function text(value) {
  return document.createTextNode(String(value));
}

function section(title, ...children) {
  const box = el("section");
  box.append(el("h2", null, null, text(title)));
  box.append(...children);
  return box;
}

draw();
