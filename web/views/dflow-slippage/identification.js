// Identification model.
//
// The graph is rendered from artifacts/analysis/route_stable_causal_model.json:
// an authored, frozen, number-free file. Node positions, edges, evidence
// classes and the interactive modes all come from that file. Nothing here
// derives an edge from data, and no edge is ever added because an observation
// looked suggestive.
//
// Fixed semantic positions, deliberately. A force-directed layout would move
// nodes for aesthetic reasons and imply structure that is not in the model,
// which is also why no graph library is used: the value here is the fixed
// arrangement, not simulation.
//
// Animation is choreography over that fixed arrangement, never new structure:
// the SVG is built once and mode changes mutate classes in place, so emphasis
// fades rather than cuts; pulses travel along exactly the edges the model's
// mode declares; and the one element that appears only under selection — the
// induced-association arc — is drawn from the model's own collider block, and
// is styled and labelled as *not* a causal edge. Everything respects
// prefers-reduced-motion.

import { el, note, prose, badge } from "../../components/format.js";
import { evidence } from "./index.js";
import { state, render } from "../../app.js";

// XML namespace literal, not a network location: createElementNS requires it.
const SVG_NS = ["http:", "//www.w3.org/2000/svg"].join("");

const NODE_W = 148;
const NODE_H = 54;

const STEP_MS = 3400;
const PULSE_MS = 1100;
const PULSE_STAGGER_MS = 260;

// Walkthrough state. Module-level so a re-render (node select, navigation)
// can cancel a run cleanly.
let playTimer = null;

function stopPlay() {
  if (playTimer) {
    clearTimeout(playTimer);
    playTimer = null;
  }
}

function reducedMotion() {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches ?? false;
}

function svgEl(tag, attrs = {}, children = []) {
  const node = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    node.setAttribute(k, String(v));
  }
  for (const child of [].concat(children)) {
    if (!child) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

/** Where an edge meets a node box, so arrows stop at the border. */
function anchor(from, to) {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const halfW = NODE_W / 2 + 4;
  const halfH = NODE_H / 2 + 4;
  const scale = Math.min(
    dx === 0 ? Infinity : Math.abs(halfW / dx),
    dy === 0 ? Infinity : Math.abs(halfH / dy),
  );
  return { x: from.x + dx * scale, y: from.y + dy * scale };
}

/** The node both collider edges point into — the shared child. Data-driven. */
function colliderChild(model) {
  const edges = (model.collider?.edges || [])
    .map((id) => (model.edges || []).find((e) => e.id === id))
    .filter(Boolean);
  if (edges.length < 2) return null;
  return edges.every((e) => e.to === edges[0].to) ? edges[0].to : null;
}

/**
 * The induced-association arc between the collider's parents. It exists in
 * the model as prose (collider.explanation); here it becomes a drawn element
 * only while a mode emphasizes every collider edge — and it is deliberately
 * arrowless, dashed and labelled, so it can never be read as a causal edge.
 */
function buildInducedArc(model, nodes) {
  const child = colliderChild(model);
  const parents = (model.collider?.nodes || []).filter((id) => id !== child);
  if (!child || parents.length !== 2) return null;

  const [a, b] = parents.map((id) => nodes.get(id));
  const c = nodes.get(child);
  if (!a || !b || !c) return null;

  // Bow the arc away from the child so it visibly bypasses R.
  const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
  const away = { x: mid.x - c.x, y: mid.y - c.y };
  const len = Math.hypot(away.x, away.y) || 1;
  const ctrl = { x: mid.x + (away.x / len) * 110, y: mid.y + (away.y / len) * 110 };

  // Label at the curve's own midpoint (quadratic Bézier at t = 0.5), so it
  // sits on the arc and stays inside the viewBox instead of at the control
  // point, which can fall off-canvas.
  const midpoint = {
    x: 0.25 * a.x + 0.5 * ctrl.x + 0.25 * b.x,
    y: 0.25 * a.y + 0.5 * ctrl.y + 0.25 * b.y,
  };

  const group = svgEl("g", { class: "dag-induced-group is-hidden", "aria-hidden": "true" });
  group.append(
    svgEl("path", {
      d: `M ${a.x} ${a.y} Q ${ctrl.x} ${ctrl.y} ${b.x} ${b.y}`,
      class: "dag-induced",
    }),
  );
  group.append(
    svgEl(
      "text",
      { x: midpoint.x, y: midpoint.y - 10, "text-anchor": "middle", class: "dag-induced-label" },
      ["induced by selection — not a causal edge"],
    ),
  );
  return group;
}

/** Build the SVG once, neutrally; applyMode() mutates it in place. */
function drawGraph(model, selected) {
  const nodes = new Map(model.nodes.map((n) => [n.id, n]));

  const maxX = Math.max(...model.nodes.map((n) => n.x), model.boundary?.x || 0) + 150;
  const maxY = Math.max(...model.nodes.map((n) => n.y)) + 90;

  const svg = svgEl("svg", {
    class: "dag",
    viewBox: `0 0 ${maxX} ${maxY}`,
    role: "img",
    "aria-labelledby": "dag-title",
    "aria-describedby": "dag-desc",
  });
  svg.append(
    svgEl("title", { id: "dag-title" }, ["Working identification model"]),
    svgEl(
      "desc",
      { id: "dag-desc" },
      [
        "Structural graph: assigned slippage S and unmeasured market state U both point to observed route R. Response quantities and the constructed transaction sit downstream. Settlement is beyond the observation boundary. Selection on a stable route is not an intervention on route.",
      ],
    ),
  );

  const defs = svgEl("defs");
  for (const [id, cls] of [
    ["arrow", "dag-arrow"],
    ["arrow-emph", "dag-arrow is-emphasized"],
  ]) {
    defs.append(
      svgEl(
        "marker",
        {
          id,
          viewBox: "0 0 10 10",
          refX: "9",
          refY: "5",
          markerWidth: "6",
          markerHeight: "6",
          orient: "auto-start-reverse",
        },
        // stroke is cleared inline: a marker does not inherit the referencing
        // line's stroke, and a stroked arrowhead reads as a hollow triangle.
        [svgEl("path", { d: "M 0 1 L 10 5 L 0 9 z", class: cls, stroke: "none" })],
      ),
    );
  }
  svg.append(defs);

  // Observation boundary, straight from the model.
  if (model.boundary) {
    svg.append(
      svgEl("line", {
        x1: model.boundary.x,
        y1: 20,
        x2: model.boundary.x,
        y2: maxY - 30,
        class: "dag-boundary",
      }),
    );
    svg.append(
      svgEl("text", { x: model.boundary.x + 8, y: 32, class: "dag-label" }, [
        model.boundary.label,
      ]),
    );
  }

  for (const edge of model.edges) {
    const from = nodes.get(edge.from);
    const to = nodes.get(edge.to);
    if (!from || !to) continue;
    const a = anchor(from, to);
    const b = anchor(to, from);
    svg.append(
      svgEl("line", {
        x1: a.x,
        y1: a.y,
        x2: b.x,
        y2: b.y,
        class: `dag-edge edge-${edge.evidence_class}`,
        "data-edge": edge.id,
        "marker-end": "url(#arrow)",
      }),
    );
  }

  const induced = buildInducedArc(model, nodes);
  if (induced) svg.append(induced);

  for (const node of model.nodes) {
    const group = svgEl("g", {
      class: "dag-node selectable",
      "data-node": node.id,
      tabindex: "0",
      role: "button",
      "aria-label": `${node.id}: ${node.name}`,
    });
    group.append(
      svgEl("rect", {
        x: node.x - NODE_W / 2,
        y: node.y - NODE_H / 2,
        width: NODE_W,
        height: NODE_H,
      }),
    );
    group.append(svgEl("text", { x: node.x, y: node.y - 4, "text-anchor": "middle" }, [node.id]));
    group.append(
      svgEl("text", { x: node.x, y: node.y + 14, "text-anchor": "middle", class: "dag-sub" }, [
        node.name,
      ]),
    );
    // The one assigned quantity carries its do(·) permanently: it is true in
    // every mode, and it is what the whole intervention/selection contrast
    // hangs on. Read from the node's role, never hard-coded to an id.
    if (node.role === "intervened") {
      group.append(
        svgEl(
          "text",
          { x: node.x, y: node.y - NODE_H / 2 - 7, "text-anchor": "middle", class: "dag-do" },
          ["do( · ) — assigned"],
        ),
      );
    }
    const select = () => {
      stopPlay();
      state.quantity = node.id;
      render();
    };
    group.addEventListener("click", select);
    group.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        select();
      }
    });
    if (selected === node.id) group.classList.add("is-emphasized");
    svg.append(group);
  }

  return svg;
}

/** Remove any in-flight pulse dots. */
function clearPulses(svg) {
  for (const dot of svg.querySelectorAll(".dag-pulse")) dot.remove();
}

/**
 * Send a dot travelling along each emphasized edge, in the order the model
 * lists them, staggered so the eye can follow the flow. Pure choreography:
 * which edges pulse is the mode's own emphasize list.
 */
function pulseEdges(svg, edgeIds) {
  // Element.animate is the only Web Animations call on the site; degrade to a
  // still diagram rather than throwing if it is unavailable.
  if (reducedMotion() || !edgeIds.length || typeof Element.prototype.animate !== "function") {
    return;
  }
  clearPulses(svg);
  edgeIds.forEach((id, i) => {
    const line = svg.querySelector(`[data-edge="${id}"]`);
    if (!line) return;
    const x1 = Number(line.getAttribute("x1"));
    const y1 = Number(line.getAttribute("y1"));
    const x2 = Number(line.getAttribute("x2"));
    const y2 = Number(line.getAttribute("y2"));
    const dot = svgEl("circle", { r: "4", class: "dag-pulse", "aria-hidden": "true" });
    svg.append(dot);
    const animation = dot.animate(
      [
        { transform: `translate(${x1}px, ${y1}px)`, opacity: 0 },
        { transform: `translate(${x1}px, ${y1}px)`, opacity: 1, offset: 0.08 },
        { transform: `translate(${x2}px, ${y2}px)`, opacity: 1, offset: 0.92 },
        { transform: `translate(${x2}px, ${y2}px)`, opacity: 0 },
      ],
      { duration: PULSE_MS, delay: i * PULSE_STAGGER_MS, iterations: 2, easing: "ease-in-out" },
    );
    animation.onfinish = () => dot.remove();
  });
}

/**
 * Apply a mode to the already-built SVG. Class toggles, not a rebuild, so the
 * CSS transitions carry the change — the dimming and thickening is the
 * animation, and a hard cut never happens.
 */
function applyMode(svg, model, mode, summaryP, { pulse = true } = {}) {
  const emphasize = new Set(mode.emphasize || []);
  const dim = new Set(mode.dim || []);
  const hasEmphasis = emphasize.size > 0;

  for (const line of svg.querySelectorAll("[data-edge]")) {
    const id = line.getAttribute("data-edge");
    const isEmphasized = emphasize.has(id);
    line.classList.toggle("is-emphasized", isEmphasized);
    // With an emphasis set active, everything not in it recedes; the dim list
    // still forces specific edges down in modes with no emphasis of their own.
    line.classList.toggle("is-dim", dim.has(id) || (hasEmphasis && !isEmphasized));
    line.setAttribute("marker-end", `url(#${isEmphasized ? "arrow-emph" : "arrow"})`);
  }

  for (const group of svg.querySelectorAll("[data-node]")) {
    const id = group.getAttribute("data-node");
    group.classList.toggle("is-emphasized", emphasize.has(id));
    group.classList.toggle("is-dim", dim.has(id));
  }

  // Selection choreography, entirely from the model's collider block: when a
  // mode emphasizes every collider edge, the shared child reads as
  // conditioned-on and the induced (non-causal) association appears.
  const colliderEdges = model.collider?.edges || [];
  const selecting =
    colliderEdges.length > 0 && colliderEdges.every((id) => emphasize.has(id));
  const child = colliderChild(model);
  if (child) {
    svg
      .querySelector(`[data-node="${child}"]`)
      ?.classList.toggle("is-conditioned", selecting);
  }
  svg.querySelector(".dag-induced-group")?.classList.toggle("is-hidden", !selecting);

  svg.setAttribute("aria-label", `Identification model, ${mode.label} view`);
  summaryP.textContent = mode.summary;

  if (pulse) pulseEdges(svg, mode.emphasize || []);
  else clearPulses(svg);
}

function legend() {
  const items = [
    ["intervention_supported", "assigned / supported"],
    ["observed_covariation", "observed covariation"],
    ["hypothesized", "hypothesized"],
    ["candidate", "candidate"],
    ["unobserved", "never observed"],
  ];
  const wrap = el("div", { class: "legend" });
  for (const [cls, label] of items) {
    const svg = svgEl("svg", { width: "34", height: "10", "aria-hidden": "true" }, [
      svgEl("line", { x1: 1, y1: 5, x2: 33, y2: 5, class: `dag-edge edge-${cls}` }),
    ]);
    wrap.append(el("span", {}, [svg, el("span", { text: label })]));
  }
  return wrap;
}

export function renderIdentification(root) {
  stopPlay();
  const { causal, extract, batches } = evidence();
  if (!causal) {
    root.append(note("The causal model artifact is not available in this deployment."));
    return;
  }

  root.append(el("h2", { text: "Working identification model" }));
  // One badge, not three: "frozen" and the model id restate what the line
  // below already says in words, and the id is implementation metadata on a
  // reading path. It stays in the artifact for anyone who needs it.
  root.append(
    el("div", { class: "badges" }, [
      badge("structural assumptions, not inferred causal edges", "type"),
    ]),
  );
  root.append(
    prose(
      "Authored and frozen before results, not fitted: no probabilities, no effect sizes, no edge drawn because the data suggested it.",
    ),
  );

  const modes = causal.modes || [];
  const initialMode = modes.find((m) => m.id === state.mode) || modes[0];

  const svg = drawGraph(causal, state.quantity);
  const summaryP = el("p", { class: "prose" });

  const modeButtons = new Map();
  const setPressed = (id) => {
    for (const [mid, button] of modeButtons) {
      button.setAttribute("aria-pressed", String(mid === id));
    }
  };

  const selectMode = (mode, opts) => {
    state.mode = mode.id;
    setPressed(mode.id);
    applyMode(svg, causal, mode, summaryP, opts);
  };

  // ▶ play: step through the model's own modes in order, so the diagram tells
  // its story — structure, then what was set, then what was selected on, then
  // what the run actually supports.
  let playButton;
  const stepThrough = (index) => {
    playTimer = null;
    if (!svg.isConnected) return;
    if (index >= modes.length) {
      playButton.textContent = "▶ play";
      return;
    }
    selectMode(modes[index], { pulse: true });
    playTimer = setTimeout(() => stepThrough(index + 1), STEP_MS);
  };
  playButton = el("button", {
    class: "control control-play",
    type: "button",
    text: "▶ play",
    "aria-label": "Play the walkthrough of all views",
    onclick: () => {
      if (playTimer) {
        stopPlay();
        playButton.textContent = "▶ play";
      } else {
        playButton.textContent = "⏹ stop";
        stepThrough(0);
      }
    },
  });

  const controls = el("div", { class: "controls" }, [playButton]);
  for (const m of modes) {
    const button = el("button", {
      class: "control",
      type: "button",
      text: m.label,
      "aria-pressed": String(m.id === initialMode.id),
      onclick: () => {
        stopPlay();
        playButton.textContent = "▶ play";
        selectMode(m, { pulse: true });
      },
    });
    modeButtons.set(m.id, button);
    controls.append(button);
  }
  root.append(controls);

  root.append(el("div", { class: "dag-wrap" }, [svg]));
  root.append(legend());
  root.append(summaryP);

  // Settle the initial mode without motion; motion belongs to interaction.
  selectMode(initialMode, { pulse: false });

  // Node detail, from the model's own definitions.
  const node = (causal.nodes || []).find((n) => n.id === state.quantity);
  if (node) {
    root.append(
      el("div", { class: "card" }, [
        el("h4", { text: `${node.id} — ${node.name}` }),
        el("p", { text: node.definition }),
        el("div", { class: "badges" }, [badge(`role: ${node.role}`)]),
      ]),
    );
  } else {
    root.append(note("Select a node for its definition, or press play for the walkthrough."));
  }

  // Progressive disclosure of the selection-vs-intervention argument.
  // Formulas are plain ASCII from the authored model — HTML/CSS/JS only.
  // No intro paragraph: the heading names the claim and step 1 states it.
  root.append(el("h3", { text: "SELECTION != INTERVENTION" }));

  const steps = causal.collider?.notation_steps || [];
  if (steps.length) {
    let stepIndex = 0;
    const panel = el("div", {
      class: "notation-walk",
      role: "region",
      "aria-label": "Selection versus intervention walkthrough",
    });
    const status = el("p", { class: "notation-status", "aria-live": "polite" });
    const list = el("ol", { class: "notation-steps" });
    const prevBtn = el("button", {
      class: "control",
      type: "button",
      text: "← previous",
      "aria-label": "Previous notation step",
    });
    const nextBtn = el("button", {
      class: "control",
      type: "button",
      text: "next →",
      "aria-label": "Next notation step",
    });
    const stepControls = el("div", { class: "controls notation-controls" }, [
      prevBtn,
      nextBtn,
    ]);

    const paint = () => {
      list.replaceChildren();
      for (let i = 0; i <= stepIndex; i += 1) {
        const step = steps[i];
        const item = el("li", {
          class: `notation-step${i === stepIndex ? " is-current" : ""}`,
        });
        for (const block of step.formulas || []) {
          const blockEl = el("div", { class: "notation-block" });
          blockEl.append(el("pre", { class: "notation", text: block.expr }));
          if (block.role) {
            blockEl.append(el("p", { class: "notation-role", text: block.role }));
          }
          blockEl.append(el("p", { class: "prose", text: block.plain }));
          item.append(blockEl);
        }
        list.append(item);
      }
      status.textContent = `Step ${stepIndex + 1} of ${steps.length}`;
      prevBtn.disabled = stepIndex === 0;
      nextBtn.disabled = stepIndex >= steps.length - 1;
    };

    prevBtn.addEventListener("click", () => {
      if (stepIndex > 0) {
        stepIndex -= 1;
        paint();
      }
    });
    nextBtn.addEventListener("click", () => {
      if (stepIndex < steps.length - 1) {
        stepIndex += 1;
        paint();
      }
    });
    panel.tabIndex = 0;
    panel.addEventListener("keydown", (e) => {
      if (e.key === "ArrowRight" || e.key === "ArrowDown") {
        e.preventDefault();
        nextBtn.click();
      } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
        e.preventDefault();
        prevBtn.click();
      }
    });

    panel.append(status, stepControls, list);
    root.append(panel);
    paint();
  }

  if (extract) {
    root.append(
      el("p", { class: "prose" }, [
        `Selecting on R kept ${extract.eligible_batch_count} of ${extract.total_batches} brackets — a cleaner forensic window, not a controlled direct effect. Conditioning on a shared child can associate its parents; the curved mark in the selection view is that induced association, not a new causal edge.`,
      ]),
    );
  }

  // No "full reasoning" disclosure here on purpose. The stepper already walks
  // the argument one formula at a time in plain language, and the anti-claim
  // ("not a controlled direct effect") is stated by the notation, the takeaway
  // above and the ceiling below. A fourth copy was flooding, not rigour.
  // The authored terminology guidance lives in the model file for writers.

  // The anchor-agreement case: rendered only if the projection is present.
  const rejectedWithMatchingAnchors = (batches?.batches || []).filter(
    (b) => b.route_class_a1_a2 === "exact_route_stable" && !b.eligible_for_instruction_diff,
  );
  if (rejectedWithMatchingAnchors.length) {
    const example = rejectedWithMatchingAnchors[0];
    root.append(el("h3", { text: "why anchor agreement is not enough" }));
    root.append(
      el("p", { class: "prose" }, [
        `Bracket ${String(example.batch_index).padStart(2, "0")} returned identical anchors and was still rejected: the treatment request routed elsewhere. `,
        el("a", {
          href: `#/explore/dflow-slippage/route?batch=${example.batch_index}`,
          text: "See the bracket →",
        }),
      ]),
    );
  }

  root.append(
    el("p", { class: "note warn" }, [
      el("strong", { text: "Ceiling: " }),
      "this model is an assumption record. Verification never reads it, no edge is derived from observed data, and nothing on this page should be described as a measured causal effect.",
    ]),
  );
}
