// Load your own lineage.
//
// Browser-local only: the File API reads the file into memory in the tab. There
// is no upload, no fetch, no analytics, no storage. Nothing in this file sends
// bytes anywhere, and a guard test asserts that.

import { el, note, errorBox, definitionList } from "./format.js";

/** Schema versions this viewer knows how to render. */
export const SUPPORTED = {
  lineage: ["1.0.0"],
  context: ["1.0.0"],
  verification: ["1.0.0"],
};

/**
 * Identify a parsed JSON document. Structural only — this does not validate
 * empirical content, which is Rust's job.
 */
export function classify(doc) {
  if (!doc || typeof doc !== "object") return { kind: "unknown" };
  if (Array.isArray(doc.results) && doc.summary) return { kind: "verification", version: doc.schema_version };
  if (doc.capture && doc.quote && doc.transaction_construction) {
    return { kind: "lineage", version: doc.schema_version };
  }
  if (doc.provenance && doc.provider && doc.schema_version) {
    return { kind: "context", version: doc.schema_version };
  }
  if (doc.threshold_identity && doc.candidate_result) {
    return { kind: "evidence_extract", version: doc.schema_version };
  }
  return { kind: "unknown" };
}

/** Returns `{ kind, doc }` or throws with a message meant to be read. */
export function accept(text, filename) {
  let doc;
  try {
    doc = JSON.parse(text);
  } catch (e) {
    throw new Error(`${filename} is not valid JSON (${e.message}).`);
  }

  const { kind, version } = classify(doc);
  if (kind === "unknown") {
    throw new Error(
      `${filename} is not a recognised artifact. Expected a lineage.json, context.json or verification.json produced by onchain-execution-lineage.`,
    );
  }
  if (kind === "evidence_extract") {
    throw new Error(
      `${filename} is a publication evidence extract, not a lineage. Open the reference case view instead.`,
    );
  }
  const allowed = SUPPORTED[kind];
  if (allowed && !allowed.includes(version)) {
    throw new Error(
      `${filename} declares ${kind} schema_version "${version}", which this viewer does not support (supported: ${allowed.join(", ")}). Regenerate it with a matching CLI build.`,
    );
  }
  return { kind, doc };
}

export function renderLoader(root, state, onLoaded) {
  const disclosure = el("details", { class: "disclosure" }, [
    el("summary", { text: "Don't have a lineage.json yet?" }),
    el("pre", {
      text: [
        "# 1. build the lineage from a raw provider response",
        "onchain-execution-lineage extract \\",
        "  --provider dflow \\",
        "  --response capture.json \\",
        "  --out-dir ./my-lineage",
        "",
        "# 2. run the cross-layer checks",
        "onchain-execution-lineage verify \\",
        "  --lineage ./my-lineage \\",
        "  --out-json ./my-lineage/verification.json",
        "",
        "# 3. drop ./my-lineage/*.json onto this page",
      ].join("\n"),
    }),
  ]);
  root.append(disclosure);

  const status = el("div");
  const zone = el(
    "div",
    {
      class: "dropzone",
      tabindex: "0",
      role: "button",
      "aria-label": "Drop lineage JSON files here, or activate to choose files",
    },
    [
      el("p", { text: "Drop lineage.json / context.json / verification.json here" }),
      el("p", { class: "mono", text: "or choose files" }),
    ],
  );

  const input = el("input", {
    type: "file",
    accept: "application/json,.json",
    multiple: true,
    style: "display:none",
  });

  async function handleFiles(fileList) {
    status.replaceChildren();
    const loaded = {};
    const problems = [];
    for (const file of fileList) {
      try {
        const { kind, doc } = accept(await file.text(), file.name);
        loaded[kind] = doc;
      } catch (e) {
        problems.push(e.message);
      }
    }
    for (const message of problems) status.append(errorBox(message));
    if (Object.keys(loaded).length) {
      status.append(
        definitionList([
          ["loaded", Object.keys(loaded).join(", ")],
          ["stayed local", "yes — nothing was uploaded"],
        ]),
      );
      if (!loaded.context && loaded.lineage) {
        status.append(
          note(
            "No context.json was supplied. The lineage renders, but stage provenance and the normalized execution context come from that file.",
          ),
        );
      }
      onLoaded(loaded);
    }
  }

  zone.addEventListener("click", () => input.click());
  zone.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      input.click();
    }
  });
  zone.addEventListener("dragover", (e) => {
    e.preventDefault();
    zone.classList.add("dragging");
  });
  zone.addEventListener("dragleave", () => zone.classList.remove("dragging"));
  zone.addEventListener("drop", (e) => {
    e.preventDefault();
    zone.classList.remove("dragging");
    handleFiles([...e.dataTransfer.files]);
  });
  input.addEventListener("change", () => handleFiles([...input.files]));

  root.append(
    zone,
    input,
    el("p", {
      class: "prose muted",
      style: "margin-top: 8px",
      text: "The file never leaves your browser — nothing is uploaded.",
    }),
    status,
  );

  if (state.source === "local") {
    root.append(note("A locally loaded artifact is currently being viewed. Reload the page to return to the bundled sample."));
  }
}
