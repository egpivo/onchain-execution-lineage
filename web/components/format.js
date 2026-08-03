// Display-only helpers.
//
// EVERYTHING in this file is presentation: DOM construction, text truncation,
// digit grouping, glyph selection. No empirical value is computed here, and
// nothing in this file may derive a status, a relationship, an offset or an
// amount — those arrive already decided from Rust.

/** Minimal element builder. `attrs.text` sets textContent (never innerHTML). */
export function el(tag, attrs = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === undefined || value === null || value === false) continue;
    if (key === "text") node.textContent = String(value);
    else if (key === "class") node.className = value;
    else if (key === "dataset") Object.assign(node.dataset, value);
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value === true ? "" : String(value));
  }
  for (const child of [].concat(children)) {
    if (child === null || child === undefined || child === false) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

/**
 * Status vocabulary. Colour is never the only signal: every status also has a
 * distinct glyph, its literal name as text, and a distinct border style in CSS.
 */
const STATUS = {
  PASS: { glyph: "✓", label: "PASS" },
  FAIL: { glyph: "✕", label: "FAIL" },
  CANDIDATE: { glyph: "◈", label: "CANDIDATE" },
  UNKNOWN: { glyph: "?", label: "UNKNOWN" },
  NOT_APPLICABLE: { glyph: "–", label: "NOT APPLICABLE" },
};

export function statusKey(status) {
  return String(status || "UNKNOWN").toUpperCase();
}

export function statusBadge(status) {
  const key = statusKey(status);
  const spec = STATUS[key] || STATUS.UNKNOWN;
  return el("span", { class: `status status-${key.toLowerCase()}`, title: spec.label }, [
    el("span", { class: "glyph", "aria-hidden": "true", text: spec.glyph }),
    el("span", { text: spec.label }),
  ]);
}

/** Stage observation states, same no-colour-only rule. */
const STAGE_STATE = {
  observed: { glyph: "●", label: "observed" },
  candidate: { glyph: "◈", label: "candidate" },
  unknown: { glyph: "?", label: "unknown" },
  not_applicable: { glyph: "–", label: "not applicable" },
};

export function stageBadge(state) {
  const spec = STAGE_STATE[state] || STAGE_STATE.unknown;
  return el("span", { class: `status status-${state === "observed" ? "pass" : state}` }, [
    el("span", { class: "glyph", "aria-hidden": "true", text: spec.glyph }),
    el("span", { text: spec.label }),
  ]);
}

/** Group digits for readability. Amounts stay strings — never parsed as floats. */
export function groupDigits(value) {
  const s = String(value ?? "");
  if (!/^\d+$/.test(s)) return s;
  return s.replace(/\B(?=(\d{3})+(?!\d))/g, " ");
}

export function shorten(value, head = 6, tail = 6) {
  const s = String(value ?? "");
  return s.length <= head + tail + 3 ? s : `${s.slice(0, head)}…${s.slice(-tail)}`;
}

/**
 * An account address. Truncated head/tail for scanning, full value in the
 * title, and `user-select: all` so one click takes the whole key — addresses
 * are copied far more often than they are read.
 */
export function addr(value, { full = false } = {}) {
  const s = String(value ?? "");
  if (!s) return el("span", { class: "muted", text: "—" });
  return el("span", {
    class: "addr",
    title: s,
    text: full ? s : shorten(s, 8, 8),
  });
}

/** A transaction or content hash: quieter than an address, same copy affordance. */
export function hash(value, { full = false } = {}) {
  const s = String(value ?? "");
  if (!s) return el("span", { class: "muted", text: "—" });
  return el("span", { class: "hash", title: s, text: full ? s : shorten(s, 10, 6) });
}

/** A route fingerprint — short, and compared by eye, so it gets tracking. */
export function fingerprint(value) {
  const s = String(value ?? "");
  return s ? el("span", { class: "fp", title: s, text: s }) : el("span", { class: "muted", text: "—" });
}

/**
 * A venue or protocol label. Deliberately never status-coloured: a venue is a
 * label, not a verdict, and the two must not be confusable.
 */
export function venue(value) {
  const s = String(value ?? "");
  return s ? el("span", { class: "venue", text: s }) : el("span", { class: "muted", text: "—" });
}

/** A right-aligned numeric table cell. Formatting only — never a computation. */
export function numCell(value) {
  return el("td", { class: "mono num", text: value === undefined ? "—" : String(value) });
}

/** A right-aligned numeric column header. */
export function numHeader(label) {
  return { label, num: true };
}

export function definitionList(pairs) {
  const dl = el("dl", { class: "kv" });
  for (const [term, value] of pairs) {
    if (value === undefined || value === null || value === "") continue;
    dl.append(el("dt", { text: term }));
    dl.append(
      value instanceof Node ? el("dd", {}, [value]) : el("dd", { class: "mono", text: value }),
    );
  }
  return dl;
}

export function chips(values) {
  const wrap = el("div", { class: "chips" });
  for (const v of values || []) wrap.append(el("span", { class: "chip", text: v }));
  return wrap;
}

/**
 * `headers` accepts plain strings, or `{ label, num }` to right-align a numeric
 * column. Rows are built by the caller.
 */
export function table(headers, rows) {
  const thead = el("thead", {}, [
    el(
      "tr",
      {},
      headers.map((h) =>
        typeof h === "string"
          ? el("th", { text: h })
          : el("th", { class: h.num ? "num" : undefined, text: h.label }),
      ),
    ),
  ]);
  const tbody = el("tbody", {}, rows);
  return el("div", { class: "scroll-x" }, [el("table", {}, [thead, tbody])]);
}

export function note(text) {
  return el("p", { class: "note", text });
}

export function errorBox(message) {
  return el("div", { class: "error" }, [el("strong", { text: "Cannot render this file. " }), message]);
}

export function stageLabel(stage) {
  return String(stage || "")
    .replace(/_/g, " ")
    .toUpperCase();
}

/** Anchor helper: `link("#/x", "Label")`. */
export function link(href, text, attrs = {}) {
  return el("a", { href, text, ...attrs });
}

export function badge(text, kind) {
  return el("span", { class: kind ? `badge badge-${kind}` : "badge", text });
}

/** A single figure. `value` is always pre-formatted; never computed here. */
export function metric(label, value) {
  return el("div", {}, [
    el("span", { class: "metric-value", text: value }),
    el("span", { class: "metric-label", text: label }),
  ]);
}

/** Tab strip. `items` is `[label, href, isCurrent]`. */
export function subnav(items) {
  return el(
    "nav",
    { class: "subnav" },
    items.map(([label, href, current]) =>
      el("a", { href, text: label, "aria-current": current ? "page" : undefined }),
    ),
  );
}

export function section(title, ...children) {
  return el("section", {}, [el("h2", { text: title }), ...children.flat()]);
}

export function prose(...paragraphs) {
  return el(
    "div",
    { class: "prose" },
    paragraphs.flat().map((p) => (p instanceof Node ? p : el("p", { text: p }))),
  );
}

/** Preformatted ASCII diagram. Text only — never markup. */
export function ascii(text) {
  return el("pre", { class: "ascii", text });
}

/**
 * Progressive disclosure for supporting detail: full per-request tables,
 * cross-checks, source listings. The insight stays on the page; the record
 * behind it opens on demand. `opts.open` starts it expanded — used when a
 * deep link targets content that lives inside.
 */
export function details(summaryText, children, opts = {}) {
  return el("details", { class: "disclosure", open: opts.open || undefined }, [
    el("summary", { text: summaryText }),
    ...[].concat(children),
  ]);
}
