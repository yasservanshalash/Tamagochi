// Sprite Studio — developer tool for regenerating a character's sprites from
// its own specs. Modes:
//   - Classic set    : the flat sprites/ catalogue (existing files)
//   - Modular parts   : the flat modular_sheet/sheets/ files
//   - Batches (sheets): the flux pack's real workflow — each slot has batches,
//                       a batch is a 4×2 SHEET of variant cells with its own
//                       prompt (GLOBAL_FLUX + batch). Generate the sheet, slice
//                       it into cells, accept the good variants.
//
// Pure DOM (no framework). All I/O goes through the typed bridge.

import {
  devMode,
  spriteSpecs,
  listSprites,
  batches,
  referenceSheet,
  assembly,
  rig,
  slotPrompt,
  imageModels,
  generateSprite,
  acceptSprite,
  sliceAccept,
  regionAccept,
  stateAccept,
  type SpriteSet,
  type Specs,
  type SpriteEntry,
  type Batch,
  type Assembly,
  type Rig,
} from "../bridge";

type Mode = "classic" | "modular" | "batches" | "builder";

// --- tiny DOM helper --------------------------------------------------------
type Attrs = Record<string, unknown>;
function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  kids: (Node | string)[] = [],
): HTMLElementTagNameMap[K] {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null) continue;
    if (k === "class") n.className = String(v);
    else if (k === "html") (n as HTMLElement).innerHTML = String(v);
    else if (k.startsWith("on") && typeof v === "function")
      n.addEventListener(k.slice(2), v as EventListener);
    else n.setAttribute(k, String(v));
  }
  for (const c of kids) n.append(c);
  return n;
}
const $ = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;

// --- state ------------------------------------------------------------------
let mode: Mode = "classic";
let specs: Specs | null = null;
let sprites: SpriteEntry[] = [];
let batchList: Batch[] = [];
let refSheet: string | null = null;
let selectedId: string | null = null; // flat sprite id OR "slot/batch_id"
let filter = "";

// Builder state
let asm: Assembly | null = null;
const sel = new Map<string, string | null>(); // slot -> chosen variant
const imgCache = new Map<string, HTMLImageElement>(); // "slot/variant" -> image
const overrides = new Map<string, { label: string; img: HTMLImageElement }>(); // live "try" parts
const BUILDER_SCALE = 2;
// Preferred default variant per slot (falls back to the first available).
const PREFERRED = ["front", "idle_front", "stand", "neutral", "default_front", "on_ears_front", "relaxed", "standing", "flat", "neutral_front"];

// Rig state: the skeleton is the placement authority. Parts attach to joints;
// dragging a joint (or loading a character with different dimensions) moves
// every attached part by the joint's offset from the spec default.
let rigData: Rig | null = null;
const jointNow = new Map<string, [number, number]>(); // current joint positions
const slotJoint = new Map<string, string>(); // slot -> joint name
let showRig = true;
let dragJoint: string | null = null;

// Image models come live from OpenRouter (newest first) so the picker never
// goes stale; Nano Banana 2 is the preferred default when present.
let modelList: [string, string][] = [];
const PREFERRED_MODEL = "google/gemini-3.1-flash-image";
// Hover state for the game-style part inspector on the stage.
let hoverSlot: string | null = null;

function jointOffset(slot: string): [number, number] {
  if (!rigData) return [0, 0];
  const j = slotJoint.get(slot);
  if (!j) return [0, 0];
  const def = rigData.joints[j];
  const now = jointNow.get(j);
  if (!def || !now) return [0, 0];
  return [now[0] - def[0], now[1] - def[1]];
}

const gridEl = $("#grid");
const detailEl = $("#detail");

const batchKey = (b: Batch) => `${b.slot}/${b.batch_id}`;
function thumb(dataUrl: string | null, alt = ""): HTMLElement {
  const box = el("div", { class: "thumb" });
  if (dataUrl) box.append(el("img", { src: dataUrl, alt }));
  return box;
}

// --- left grid --------------------------------------------------------------
function renderGrid() {
  if (mode === "batches") return renderBatchList();
  gridEl.innerHTML = "";
  const q = filter.trim().toLowerCase();
  const shown = sprites.filter(
    (s) => !q || s.id.toLowerCase().includes(q) || s.label.toLowerCase().includes(q),
  );
  if (!shown.length) {
    gridEl.append(el("div", { class: "hint", style: "padding:10px" }, ["No sprites match."]));
    return;
  }
  for (const s of shown) {
    gridEl.append(
      el("div", { class: "cell" + (s.id === selectedId ? " sel" : ""), onclick: () => selectFlat(s.id) }, [
        thumb(s.data_url, s.label),
        el("div", { class: "name" }, [s.label]),
      ]),
    );
  }
}

function renderBatchList() {
  gridEl.innerHTML = "";
  const q = filter.trim().toLowerCase();
  const shown = batchList.filter(
    (b) => !q || b.slot.includes(q) || b.title.toLowerCase().includes(q),
  );
  let slot = "";
  for (const b of shown) {
    if (b.slot !== slot) {
      slot = b.slot;
      gridEl.append(el("div", { class: "section" }, [el("span", { class: "s-name" }, [slot.replace(/_/g, " ")])]));
    }
    const have = b.cells.filter((c) => c.has_sprite).length;
    const cell = el(
      "div",
      {
        class: "cell badge" + (have === b.cells.length ? "" : " missing") + (batchKey(b) === selectedId ? " sel" : ""),
        style: "grid-column: 1 / -1; flex-direction: row; align-items:center; gap:10px;",
        onclick: () => selectBatch(batchKey(b)),
      },
      [
        el("span", { class: "dot " + (have ? "has" : "miss") }),
        el("div", { style: "flex:1; overflow:hidden" }, [
          el("div", { class: "name" }, [b.batch_id.replace("batch_", "batch ")]),
          el("div", { class: "hint" }, [`${have}/${b.cells.length} cells`]),
        ]),
      ],
    );
    gridEl.append(cell);
  }
  if (!shown.length) gridEl.append(el("div", { class: "hint", style: "padding:10px" }, ["No batches match."]));
}

// --- shared detail bits -----------------------------------------------------
function swatches(palette: string[]): HTMLElement {
  return el("div", { class: "swatches" }, palette.map((c) => el("span", { class: "sw", style: `background:${c}`, title: c })));
}

// --- flat detail ------------------------------------------------------------
function renderFlatDetail() {
  detailEl.innerHTML = "";
  const s = sprites.find((x) => x.id === selectedId);
  if (!s) {
    detailEl.append(el("div", { class: "empty" }, ["Pick a sprite on the left to regenerate it."]));
    return;
  }
  const meta = el("div", { class: "meta" }, [
    el("div", {}, [el("b", {}, [s.label])]),
    el("div", {}, [s.id]),
    el("div", {}, [`${s.size[0]}×${s.size[1]} px`]),
  ]);
  if (specs?.palette?.length) {
    meta.append(el("div", { style: "margin-top:6px" }, ["palette"]));
    meta.append(swatches(specs.palette));
  }
  detailEl.append(el("div", { class: "card" }, [el("h3", {}, ["Current sprite"]), el("div", { class: "cur" }, [thumb(s.data_url, s.label), meta])]));

  const promptBox = el("textarea", { class: "field" }) as HTMLTextAreaElement;
  promptBox.value = s.suggested_prompt;
  const countInput = el("input", { class: "field", type: "number", min: "1", max: "4", value: "2" }) as HTMLInputElement;
  const refToggle = el("input", { type: "checkbox" }) as HTMLInputElement;
  refToggle.checked = true;
  const genBtn = el("button", { class: "btn primary", id: "gen" }, ["Generate"]);
  genBtn.addEventListener("click", () =>
    runGenerateFlat(s, promptBox.value, Math.min(4, Math.max(1, parseInt(countInput.value || "1", 10))), refToggle.checked),
  );
  detailEl.append(
    el("div", { class: "card" }, [
      el("h3", {}, ["Regenerate from specs"]),
      promptBox,
      el("div", { class: "row" }, [genBtn, el("span", { class: "count" }, ["candidates", countInput]), el("label", { class: "count", style: "cursor:pointer" }, [refToggle, "anchor to current"])]),
      el("div", { class: "hint", style: "margin-top:8px" }, ["Candidates are not saved until you accept one."]),
    ]),
  );
  detailEl.append(candsCard());
}

function candsCard(): HTMLElement {
  return el("div", { class: "card", id: "candsCard", style: "display:none" }, [el("h3", {}, ["Candidates"]), el("div", { id: "msg" }), el("div", { class: "cands", id: "cands" })]);
}

async function runGenerateFlat(s: SpriteEntry, prompt: string, n: number, useRef: boolean) {
  const card = $("#candsCard"), msg = $("#msg"), cands = $("#cands"), genBtn = $("#gen") as HTMLButtonElement;
  card.style.display = ""; cands.innerHTML = ""; msg.innerHTML = "";
  msg.append(el("div", { class: "alert", html: `<span class="spin"></span> Generating ${n}…` }));
  genBtn.disabled = true;
  try {
    const res = await generateSprite(prompt, mode as SpriteSet, useRef ? s.id : null, n);
    msg.innerHTML = "";
    if (!res.images.length) { msg.append(el("div", { class: "alert err" }, ["No image returned."])); return; }
    for (const url of res.images) {
      const accept = el("button", { class: "btn primary" }, ["Accept & replace"]);
      accept.addEventListener("click", async () => {
        accept.disabled = true; accept.textContent = "Saving…";
        try {
          const backup = await acceptSprite(mode as SpriteSet, s.id, url);
          s.data_url = url; renderGrid();
          (detailEl.querySelector(".cur .thumb img") as HTMLImageElement | null)?.setAttribute("src", url);
          msg.append(el("div", { class: "alert ok" }, [`Replaced ${s.rel_path}. Backup: ${backup}. Relaunch to see it live.`]));
          accept.textContent = "Accepted ✓";
        } catch (e) { accept.disabled = false; accept.textContent = "Accept & replace"; msg.append(el("div", { class: "alert err" }, [String(e)])); }
      });
      cands.append(el("div", { class: "cand" }, [thumb(url, "candidate"), accept]));
    }
  } catch (e) { msg.innerHTML = ""; msg.append(el("div", { class: "alert err" }, [String(e)])); }
  finally { genBtn.disabled = false; }
}

// --- batch detail -----------------------------------------------------------
function renderBatchDetail() {
  detailEl.innerHTML = "";
  const b = batchList.find((x) => batchKey(x) === selectedId);
  if (!b) {
    detailEl.append(el("div", { class: "empty" }, ["Pick a batch. Generate its sheet, then slice the cells you like."]));
    return;
  }
  const bd = b.bounds;
  const meta = el("div", { class: "meta" }, [
    el("div", {}, [el("b", {}, [b.title])]),
    el("div", {}, [`${b.cols}×${b.rows} grid · ${b.cell_px}px cells`]),
    bd ? el("div", {}, [`bounds ${bd.x},${bd.y} · ${bd.width}×${bd.height}`]) : "",
    b.pivot ? el("div", {}, [`pivot ${b.pivot.join(",")}`]) : "",
    el("div", { style: "margin-top:6px" }, [`reference: ${refSheet ? "master sheet attached" : "none"}`]),
  ]);
  if (specs?.palette?.length) meta.append(swatches(specs.palette));

  // Cell roster
  const roster = el("div", { class: "cands" });
  for (const c of b.cells) {
    roster.append(
      el("div", { class: "cand", style: "align-items:center" }, [
        thumb(c.data_url, c.label),
        el("div", { class: "name", style: "text-align:center" }, [
          el("span", { class: "dot " + (c.has_sprite ? "has" : "miss") }),
          c.label,
        ]),
      ]),
    );
  }
  detailEl.append(el("div", { class: "card" }, [el("h3", {}, ["Batch"]), el("div", { class: "cur" }, [meta]), el("h3", { style: "margin-top:12px" }, ["Cells (variants)"]), roster]));

  // Prompt + generate
  const promptBox = el("textarea", { class: "field", style: "min-height:160px" }) as HTMLTextAreaElement;
  promptBox.value = b.prompt;
  const genBtn = el("button", { class: "btn primary", id: "gen" }, ["Generate sheet"]);
  genBtn.addEventListener("click", () => runGenerateBatch(b, promptBox.value));
  detailEl.append(
    el("div", { class: "card" }, [
      el("h3", {}, ["Prompt (GLOBAL_FLUX + this batch)"]),
      promptBox,
      el("div", { class: "row" }, [genBtn]),
      el("div", { class: "hint", style: "margin-top:8px" }, ["Generates one 4×2 sheet using the master reference. Then pick which cells to slice out and keep."]),
    ]),
  );
  detailEl.append(el("div", { class: "card", id: "sheetCard", style: "display:none" }, [el("h3", {}, ["Generated sheet"]), el("div", { id: "msg" }), el("div", { id: "sheetWrap" })]));
}

/** Client-side slice for preview: crop a sheet dataURL into per-cell dataURLs. */
function sliceCellsForPreview(sheetUrl: string, cols: number, rows: number): Promise<string[]> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      const cw = Math.floor(img.width / cols), ch = Math.floor(img.height / rows);
      const out: string[] = [];
      const canvas = document.createElement("canvas");
      canvas.width = cw; canvas.height = ch;
      const ctx = canvas.getContext("2d")!;
      for (let r = 0; r < rows; r++) {
        for (let c = 0; c < cols; c++) {
          ctx.clearRect(0, 0, cw, ch);
          ctx.drawImage(img, c * cw, r * ch, cw, ch, 0, 0, cw, ch);
          out.push(canvas.toDataURL("image/png"));
        }
      }
      resolve(out);
    };
    img.onerror = () => resolve([]);
    img.src = sheetUrl;
  });
}

async function runGenerateBatch(b: Batch, prompt: string) {
  const card = $("#sheetCard"), msg = $("#msg"), wrap = $("#sheetWrap"), genBtn = $("#gen") as HTMLButtonElement;
  card.style.display = ""; wrap.innerHTML = ""; msg.innerHTML = "";
  msg.append(el("div", { class: "alert", html: `<span class="spin"></span> Generating sheet…` }));
  genBtn.disabled = true;
  try {
    const res = await generateSprite(prompt, "modular", null, 1, refSheet);
    msg.innerHTML = "";
    const sheetUrl = res.images[0];
    if (!sheetUrl) { msg.append(el("div", { class: "alert err" }, ["No sheet returned. Try again."])); return; }
    // Preview the full sheet.
    wrap.append(el("div", { class: "thumb", style: "height:auto; max-height:300px; margin-bottom:12px" }, [el("img", { src: sheetUrl, style: "image-rendering:pixelated" })]));

    // Slice previews + selection.
    const cellUrls = await sliceCellsForPreview(sheetUrl, b.cols, b.rows);
    const picks = new Set<number>();
    const cellsGrid = el("div", { class: "cands" });
    b.cells.forEach((c) => {
      const chk = el("input", { type: "checkbox" }) as HTMLInputElement;
      chk.checked = !c.has_sprite; // default: fill the missing ones
      if (chk.checked) picks.add(c.index);
      chk.addEventListener("change", () => (chk.checked ? picks.add(c.index) : picks.delete(c.index)));
      const tryBtn = el("button", { class: "btn ghost", style: "padding:4px 8px; font-size:11px" }, ["Try live"]);
      const cellUrl = cellUrls[c.index];
      tryBtn.addEventListener("click", () => { if (cellUrl) tryInBuilder(b.slot, c.variant, cellUrl); });
      cellsGrid.append(
        el("div", { class: "cand" }, [
          thumb(cellUrl || null, c.label),
          el("label", { class: "name", style: "text-align:center; cursor:pointer; display:flex; gap:6px; align-items:center; justify-content:center" }, [chk, c.label]),
          tryBtn,
        ]),
      );
    });

    const acceptBtn = el("button", { class: "btn primary" }, ["Accept selected cells"]);
    acceptBtn.addEventListener("click", async () => {
      acceptBtn.disabled = true; acceptBtn.textContent = "Saving…";
      try {
        const chosen = b.cells.filter((c) => picks.has(c.index)).map((c) => ({ index: c.index, variant: c.variant }));
        const written = await sliceAccept({ slot: b.slot, sheet: sheetUrl, cols: b.cols, rows: b.rows, cells: chosen, bounds: b.bounds, pivot: b.pivot });
        // Reflect saved cells.
        for (const c of b.cells) if (picks.has(c.index)) { c.has_sprite = true; c.data_url = cellUrls[c.index] || c.data_url; }
        renderGrid();
        msg.append(el("div", { class: "alert ok" }, [`Saved ${written.length} part(s): ${written.join(", ")}. Manifest updated. Relaunch to see them live.`]));
        acceptBtn.textContent = "Saved ✓";
      } catch (e) { acceptBtn.disabled = false; acceptBtn.textContent = "Accept selected cells"; msg.append(el("div", { class: "alert err" }, [String(e)])); }
    });

    wrap.append(el("div", { class: "row" }, [acceptBtn]));
    wrap.append(el("div", { style: "margin-top:12px" }, [cellsGrid]));
  } catch (e) { msg.innerHTML = ""; msg.append(el("div", { class: "alert err" }, [String(e)])); }
  finally { genBtn.disabled = false; }
}

// --- Builder (live paper-doll) ----------------------------------------------
function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = reject;
    img.src = src;
  });
}

async function prepareBuilder() {
  asm = await assembly();
  if (!modelList.length) modelList = await imageModels().catch(() => []);
  if (!rigData) {
    rigData = await rig().catch(() => null);
    if (rigData) {
      slotJoint.clear();
      for (const [slot, joint] of rigData.attach) slotJoint.set(slot, joint);
      jointNow.clear();
      for (const [name, xy] of Object.entries(rigData.joints)) jointNow.set(name, [xy[0], xy[1]]);
    }
  }
  imgCache.clear();
  const jobs: Promise<void>[] = [];
  for (const sp of asm.slots) {
    for (const inst of sp.instances) {
      const key = `${sp.slot}/${inst.variant}`;
      jobs.push(loadImage(inst.data_url).then((img) => void imgCache.set(key, img)).catch(() => {}));
    }
    if (!sel.has(sp.slot)) {
      const pref = PREFERRED.find((p) => sp.instances.some((i) => i.variant === p));
      sel.set(sp.slot, pref || sp.instances[0]?.variant || null);
    }
  }
  await Promise.all(jobs);
}

function drawBuilder() {
  if (!asm) return;
  const canvas = document.getElementById("stageCanvas") as HTMLCanvasElement | null;
  if (!canvas) return;
  const [cw, ch] = asm.canvas;
  const s = BUILDER_SCALE;
  canvas.width = cw * s;
  canvas.height = ch * s;
  const ctx = canvas.getContext("2d")!;
  ctx.imageSmoothingEnabled = false;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  // Ground line at the spec's ground_y — the game floor he stands on.
  ctx.save();
  ctx.strokeStyle = "rgba(232,161,60,0.25)";
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.moveTo(0, 290 * s);
  ctx.lineTo(cw * s, 290 * s);
  ctx.stroke();
  ctx.restore();
  for (const slot of asm.order) {
    const [dx, dy] = jointOffset(slot);
    const ov = overrides.get(slot);
    if (ov) {
      ctx.drawImage(ov.img, dx * s, dy * s, cw * s, ch * s);
      continue;
    }
    const variant = sel.get(slot);
    if (!variant) continue;
    const sp = asm.slots.find((x) => x.slot === slot);
    const inst = sp?.instances.find((i) => i.variant === variant);
    if (!inst) continue;
    const img = imgCache.get(`${slot}/${variant}`);
    if (!img) continue;
    const r = inst.region;
    const cb = inst.canonical_bounds;
    if (r && cb) {
      ctx.drawImage(img, r.x, r.y, r.w, r.h, (cb.x + dx) * s, (cb.y + dy) * s, cb.width * s, cb.height * s);
    } else {
      ctx.drawImage(img, dx * s, dy * s, cw * s, ch * s);
    }
  }
  // Game-style hover inspector: outline the body ZONE under the cursor.
  if (hoverSlot) {
    const zb = zoneBounds(hoverSlot);
    if (zb) {
      ctx.save();
      ctx.strokeStyle = "#f2b45a";
      ctx.setLineDash([4, 3]);
      ctx.lineWidth = 2;
      ctx.strokeRect(zb.x * s - 2, zb.y * s - 2, zb.width * s + 4, zb.height * s + 4);
      ctx.setLineDash([]);
      const label = ` ${hoverSlot} — click to inspect parts `;
      ctx.font = "600 11px Consolas, monospace";
      const tw = ctx.measureText(label).width;
      const lx = Math.min(zb.x * s, canvas.width - tw - 8);
      const ly = Math.max(12, zb.y * s - 8);
      ctx.fillStyle = "rgba(27,19,13,0.9)";
      ctx.fillRect(lx, ly - 11, tw + 4, 15);
      ctx.fillStyle = "#f2b45a";
      ctx.fillText(label, lx + 2, ly);
      ctx.restore();
    }
  }
  // Name plate, like a character-select screen.
  ctx.save();
  ctx.font = "700 14px Consolas, monospace";
  ctx.fillStyle = "rgba(242,180,90,0.85)";
  ctx.fillText((specs?.name || "YASSER").toUpperCase(), 10, canvas.height - 10);
  ctx.restore();
  drawRigOverlay(ctx, s);
}

/** Joints that hang below `name` in the bone graph (for hierarchical drag). */
function subtree(name: string): string[] {
  if (!rigData) return [name];
  const out = [name];
  const walk = (n: string) => {
    for (const [a, b] of rigData!.bones) {
      if (a === n) {
        out.push(b);
        walk(b);
      }
    }
  };
  walk(name);
  return out;
}

function drawRigOverlay(ctx: CanvasRenderingContext2D, s: number) {
  if (!showRig || !rigData) return;
  ctx.save();
  ctx.lineWidth = 2;
  ctx.strokeStyle = "rgba(232,161,60,0.75)";
  for (const [a, b] of rigData.bones) {
    const pa = jointNow.get(a), pb = jointNow.get(b);
    if (!pa || !pb) continue;
    ctx.beginPath();
    ctx.moveTo(pa[0] * s, pa[1] * s);
    ctx.lineTo(pb[0] * s, pb[1] * s);
    ctx.stroke();
  }
  for (const [name, p] of jointNow) {
    ctx.beginPath();
    ctx.arc(p[0] * s, p[1] * s, name === dragJoint ? 6 : 4, 0, Math.PI * 2);
    ctx.fillStyle = name === dragJoint ? "#f2b45a" : "rgba(242,180,90,0.9)";
    ctx.fill();
    ctx.strokeStyle = "#1b130d";
    ctx.stroke();
  }
  ctx.restore();
}

function wireRigDrag(canvas: HTMLCanvasElement) {
  const pick = (ev: MouseEvent): string | null => {
    const rect = canvas.getBoundingClientRect();
    const x = ((ev.clientX - rect.left) / rect.width) * canvas.width / BUILDER_SCALE;
    const y = ((ev.clientY - rect.top) / rect.height) * canvas.height / BUILDER_SCALE;
    let best: string | null = null, bd = 10; // 10 canvas-px grab radius
    for (const [name, p] of jointNow) {
      const d = Math.hypot(p[0] - x, p[1] - y);
      if (d < bd) { bd = d; best = name; }
    }
    return best;
  };
  canvas.addEventListener("mousedown", (ev) => {
    dragJoint = showRig ? pick(ev) : null;
    if (dragJoint) {
      drawBuilder();
      return;
    }
    // No joint under the cursor: clicking a body zone opens its region view.
    const rect = canvas.getBoundingClientRect();
    const x = ((ev.clientX - rect.left) / rect.width) * canvas.width / BUILDER_SCALE;
    const y = ((ev.clientY - rect.top) / rect.height) * canvas.height / BUILDER_SCALE;
    const zone = partAt(x, y);
    if (zone) openZoneView(zone);
  });
  canvas.addEventListener("mousemove", (ev) => {
    const rect = canvas.getBoundingClientRect();
    const x = Math.round(((ev.clientX - rect.left) / rect.width) * canvas.width / BUILDER_SCALE);
    const y = Math.round(((ev.clientY - rect.top) / rect.height) * canvas.height / BUILDER_SCALE);
    if (dragJoint) {
      // Hierarchical drag: the whole subtree follows, like a real rig.
      const cur = jointNow.get(dragJoint);
      if (cur) {
        const ddx = x - cur[0], ddy = y - cur[1];
        for (const j of subtree(dragJoint)) {
          const p = jointNow.get(j);
          if (p) jointNow.set(j, [p[0] + ddx, p[1] + ddy]);
        }
      }
      drawBuilder();
      return;
    }
    // Hover inspector + cursor affordance.
    const overJoint = showRig && pick(ev) !== null;
    const slot = overJoint ? null : partAt(x, y);
    canvas.style.cursor = overJoint ? "grab" : slot ? "pointer" : "default";
    if (slot !== hoverSlot) {
      hoverSlot = slot;
      drawBuilder();
    }
  });
  canvas.addEventListener("mouseup", () => { if (dragJoint) { dragJoint = null; drawBuilder(); } });
  canvas.addEventListener("mouseleave", () => {
    dragJoint = null;
    if (hoverSlot) { hoverSlot = null; }
    drawBuilder();
  });
}

function applyGesture(name: string) {
  if (!rigData || !asm) return;
  const preset = rigData.gestures[name];
  if (!preset) return;
  for (const [slot, variant] of Object.entries(preset)) {
    const sp = asm.slots.find((x) => x.slot === slot);
    if (sp?.instances.some((i) => i.variant === variant)) {
      overrides.delete(slot);
      sel.set(slot, variant);
    }
    // Slots without that part yet are skipped — generate them in Batches.
  }
  drawBuilder();
  renderBuilder();
}

function renderBuilder() {
  // Left pane: layer controls. Right pane: the stage.
  gridEl.innerHTML = "";
  detailEl.innerHTML = "";
  if (!asm) return;

  const layers = el("div", { class: "layers" });
  for (const slot of asm.order) {
    const sp = asm.slots.find((x) => x.slot === slot)!;
    const has = sp.instances.length > 0;
    const ov = overrides.get(slot);
    const select = el("select", { class: "field" }) as HTMLSelectElement;
    select.append(el("option", { value: "" }, ["— none —"]));
    for (const inst of sp.instances) {
      const o = el("option", { value: inst.variant }, [inst.variant.replace(/_/g, " ")]) as HTMLOptionElement;
      if (sel.get(slot) === inst.variant) o.selected = true;
      select.append(o);
    }
    select.disabled = !has;
    select.addEventListener("change", () => {
      overrides.delete(slot); // picking a real variant clears any "try" override
      sel.set(slot, select.value || null);
      drawBuilder();
      renderBuilder();
    });
    const right: (Node | string)[] = [];
    const aiBtn = el("button", { class: "btn ghost", style: "padding:3px 8px", title: "AI edit this part" }, ["✎"]);
    aiBtn.addEventListener("click", () => void openPartEditor(slot));
    right.push(aiBtn);
    if (ov) {
      const clear = el("button", { class: "btn ghost", style: "padding:3px 8px" }, ["× trying"]);
      clear.addEventListener("click", () => { overrides.delete(slot); drawBuilder(); renderBuilder(); });
      right.push(el("span", { class: "try" }, ["● live"]), clear);
    } else if (!has) {
      right.push(el("span", { class: "hint" }, ["no parts"]));
    }
    layers.append(el("div", { class: "lrow" + (has ? "" : " empty") }, [el("span", { class: "slot" }, [slot.replace(/_/g, " ")]), select, el("span", {}, right)]));
  }
  gridEl.append(layers);

  const canvas = el("canvas", { id: "stageCanvas" });
  const resetBtn = el("button", { class: "btn ghost" }, ["Reset to defaults"]);
  resetBtn.addEventListener("click", () => {
    sel.clear(); overrides.clear();
    for (const sp of asm!.slots) {
      const pref = PREFERRED.find((p) => sp.instances.some((i) => i.variant === p));
      sel.set(sp.slot, pref || sp.instances[0]?.variant || null);
    }
    drawBuilder(); renderBuilder();
  });
  const clearOv = el("button", { class: "btn ghost" }, ["Clear all 'trying'"]);
  clearOv.addEventListener("click", () => { overrides.clear(); drawBuilder(); renderBuilder(); });

  // Rig controls: overlay toggle, joint reset, gesture presets from the spec.
  const rigBtn = el("button", { class: "btn" + (showRig ? " primary" : " ghost") }, ["Rig"]);
  rigBtn.addEventListener("click", () => { showRig = !showRig; drawBuilder(); renderBuilder(); });
  const rigReset = el("button", { class: "btn ghost" }, ["Reset rig"]);
  rigReset.addEventListener("click", () => {
    if (rigData) for (const [n, xy] of Object.entries(rigData.joints)) jointNow.set(n, [xy[0], xy[1]]);
    drawBuilder();
  });
  const gestureBtns: Node[] = [];
  if (rigData) {
    for (const name of Object.keys(rigData.gestures)) {
      const b = el("button", { class: "btn ghost", style: "padding:6px 9px; font-size:11px" }, [name.replace(/_/g, " ")]);
      b.addEventListener("click", () => applyGesture(name));
      gestureBtns.push(b);
    }
  }

  detailEl.append(
    el("div", { class: "builder-body", style: "height:100%" }, [
      el("div", { class: "stage" }, [canvas]),
      el("div", { class: "toolbar" }, [
        rigBtn,
        rigReset,
        resetBtn,
        clearOv,
        (() => {
          const b = el("button", { class: "btn primary" }, ["New state"]);
          b.addEventListener("click", () => void openStateView());
          return b;
        })(),
        (() => {
          const b = el("button", { class: "btn" }, ["Parts sheet"]);
          b.addEventListener("click", () => void openPartsSheet());
          return b;
        })(),
        ...gestureBtns,
        el("span", { class: "hint" }, ["Drag rig joints to fit a character's dimensions — attached parts follow."]),
      ]),
    ]),
  );
  wireRigDrag(canvas);
  drawBuilder();
}

// --- Part editor: pick a body part, prompt AI to change it ------------------

/** Game-style "inspect" panel: what this part is, where it sits on the rig,
 * and how much of its variant roster exists yet. */
function analysisCard(slot: string): HTMLElement {
  const sp = asm?.slots.find((x) => x.slot === slot);
  const b = batchList.find((x) => x.slot === slot);
  const joint = slotJoint.get(slot);
  const roster = batchList.filter((x) => x.slot === slot).flatMap((x) => x.cells.map((c) => c.variant));
  const haveSet = new Set((sp?.instances ?? []).map((i) => i.variant));
  const total = roster.length || haveSet.size;
  const have = roster.length ? roster.filter((v) => haveSet.has(v)).length : haveSet.size;
  const pct = total ? Math.round((have / total) * 100) : 0;

  const rows: (Node | string)[] = [
    el("div", {}, [el("b", {}, ["Analysis"])]),
    joint ? el("div", {}, [`rig joint: ${joint.replace(/_/g, " ")}`]) : "",
    b?.bounds ? el("div", {}, [`bounds ${b.bounds.x},${b.bounds.y} · ${b.bounds.width}×${b.bounds.height}`]) : "",
    b?.pivot ? el("div", {}, [`pivot ${b.pivot.join(",")}`]) : "",
    el("div", {}, [`variants: ${have}/${total} exist (${pct}%)`]),
  ];
  // Progress bar, quest-style.
  const bar = el("div", { style: "height:8px;border-radius:4px;background:var(--inset);overflow:hidden;margin-top:6px" }, [
    el("div", { style: `height:100%;width:${pct}%;background:var(--accent)` }),
  ]);
  // Existing variants as a mini gallery.
  const gallery = el("div", { class: "cands", style: "margin-top:10px" });
  for (const inst of sp?.instances ?? []) {
    gallery.append(
      el("div", { class: "cand", style: "padding:5px" }, [
        thumb(inst.data_url, inst.variant),
        el("div", { class: "name", style: "text-align:center" }, [inst.variant.replace(/_/g, " ")]),
      ]),
    );
  }
  if (!(sp?.instances ?? []).length) gallery.append(el("div", { class: "hint" }, ["No sprites yet — everything below creates the first one."]));
  return el("div", { class: "card" }, [el("div", { class: "meta" }, rows), bar, gallery]);
}
async function openPartEditor(slot: string) {
  if (!asm) return;
  const sp = asm.slots.find((x) => x.slot === slot);
  // Batch metadata gives this slot's real prompt + variant roster.
  if (!batchList.length) batchList = await batches().catch(() => []);
  if (refSheet === null) refSheet = await referenceSheet().catch(() => null);
  const slotBatches = batchList.filter((b) => b.slot === slot);

  const curVariant = sel.get(slot) || sp?.instances[0]?.variant || null;
  const curInst = sp?.instances.find((i) => i.variant === curVariant) || sp?.instances[0] || null;

  // Variant roster: batch cells first (full spec), else existing instances.
  const variants: string[] = slotBatches.length
    ? slotBatches.flatMap((b) => b.cells.map((c) => c.variant))
    : (sp?.instances.map((i) => i.variant) ?? []);

  detailEl.innerHTML = "";
  const back = el("button", { class: "btn ghost" }, ["← Builder"]);
  back.addEventListener("click", () => renderBuilder());

  // What the AI sees: the current part if it exists, else the master sheet.
  const refUrl = curInst?.data_url || refSheet;
  const refLabel = curInst ? "current part" : "master reference sheet";

  const variantSel = el("select", { class: "field", style: "max-width:220px" }) as HTMLSelectElement;
  for (const v of variants.length ? variants : ["front"]) {
    const o = el("option", { value: v }, [v.replace(/_/g, " ")]) as HTMLOptionElement;
    if (v === curVariant) o.selected = true;
    variantSel.append(o);
  }

  const modelSel = el("select", { class: "field", style: "max-width:320px" }) as HTMLSelectElement;
  if (!modelList.length) modelList = await imageModels().catch(() => []);
  for (const [id, label] of modelList) {
    const o = el("option", { value: id }, [label]) as HTMLOptionElement;
    if (id === PREFERRED_MODEL) o.selected = true;
    modelSel.append(o);
  }

  const b0 = slotBatches[0];
  const boundsLine = b0?.bounds
    ? ` Place it at canonical bounds x=${b0.bounds.x}, y=${b0.bounds.y}, width=${b0.bounds.width}, height=${b0.bounds.height} on a 320×320 transparent canvas.`
    : " Place it at its canonical position on a 320×320 transparent canvas.";
  const promptBox = el("textarea", { class: "field", style: "min-height:120px" }) as HTMLTextAreaElement;
  promptBox.value =
    `Redraw ONLY the ${slot.replace(/_/g, " ")} (variant: ${variantSel.value}) of the attached reference, keeping the exact palette, pixel density, 1px hard outline and proportions.` +
    boundsLine +
    ` Change: <describe what you want different>.`;
  variantSel.addEventListener("change", () => {
    promptBox.value = promptBox.value.replace(/\(variant: [^)]*\)/, `(variant: ${variantSel.value})`);
  });
  // The ultimate pack's per-part prompt is the default source of truth; offer
  // it as one click rather than burying the user's short edit prompt.
  const packText = await slotPrompt(slot).catch(() => null);
  const usePack = el("button", { class: "btn ghost" }, ["Use pack prompt"]);
  usePack.addEventListener("click", () => {
    if (packText) promptBox.value = packText;
  });
  if (!packText) (usePack as HTMLButtonElement).disabled = true;

  const genBtn = el("button", { class: "btn primary", id: "gen" }, ["Generate"]);
  const nInput = el("input", { class: "field", type: "number", min: "1", max: "4", value: "2" }) as HTMLInputElement;

  detailEl.append(
    el("div", { class: "card" }, [
      el("div", { class: "row", style: "margin-top:0" }, [back, el("h3", { style: "margin:0" }, [`AI edit — ${slot.replace(/_/g, " ")}`])]),
      el("div", { class: "cur", style: "margin-top:10px" }, [
        thumb(curInst?.data_url || null, slot),
        el("div", { class: "meta" }, [
          el("div", {}, [el("b", {}, [curInst ? `current: ${curInst.variant}` : "no sprite yet — using master sheet as reference"])]),
          el("div", {}, [`reference sent to the model: ${refLabel}`]),
          el("div", { class: "row" }, ["target variant", variantSel]),
          el("div", { class: "row" }, ["model", modelSel]),
        ]),
      ]),
    ]),
    analysisCard(slot),
    el("div", { class: "card" }, [
      el("h3", {}, ["Prompt"]),
      promptBox,
      el("div", { class: "row" }, [genBtn, el("span", { class: "count" }, ["candidates", nInput]), usePack]),
      el("div", { class: "hint", style: "margin-top:8px" }, [
        "The master reference is attached automatically. Accept writes the part at its canonical bounds and updates the manifest.",
      ]),
    ]),
    candsCard(),
  );

  genBtn.addEventListener("click", async () => {
    const card = $("#candsCard"), msg = $("#msg"), cands = $("#cands");
    card.style.display = ""; cands.innerHTML = ""; msg.innerHTML = "";
    const n = Math.min(4, Math.max(1, parseInt(nInput.value || "1", 10)));
    msg.append(el("div", { class: "alert", html: `<span class="spin"></span> Generating ${n} with ${modelSel.value}…` }));
    genBtn.disabled = true;
    try {
      const res = await generateSprite(promptBox.value, "modular", null, n, refUrl, modelSel.value);
      msg.innerHTML = "";
      if (!res.images.length) { msg.append(el("div", { class: "alert err" }, ["No image returned — try again or switch model."])); return; }
      for (const url of res.images) {
        const tryBtn = el("button", { class: "btn ghost" }, ["Try live"]);
        tryBtn.addEventListener("click", () => tryInBuilder(slot, variantSel.value, url));
        const acceptBtn = el("button", { class: "btn primary" }, ["Accept"]);
        acceptBtn.addEventListener("click", async () => {
          acceptBtn.disabled = true; acceptBtn.textContent = "Saving…";
          try {
            const written = await sliceAccept({
              slot, sheet: url, cols: 1, rows: 1,
              cells: [{ index: 0, variant: variantSel.value }],
              bounds: b0?.bounds ?? null, pivot: b0?.pivot ?? null,
            });
            msg.append(el("div", { class: "alert ok" }, [`Saved ${written.join(", ")}. Reopen Builder to select it; relaunch to see it live.`]));
            acceptBtn.textContent = "Accepted ✓";
            asm = await assembly(); // refresh so the builder sees the new part
          } catch (e) { acceptBtn.disabled = false; acceptBtn.textContent = "Accept"; msg.append(el("div", { class: "alert err" }, [String(e)])); }
        });
        cands.append(el("div", { class: "cand" }, [thumb(url, "candidate"), el("div", { class: "row", style: "margin-top:0" }, [tryBtn, acceptBtn])]));
      }
    } catch (e) { msg.innerHTML = ""; msg.append(el("div", { class: "alert err" }, [String(e)])); }
    finally { genBtn.disabled = false; }
  });
}

// Coarse body zones: hover shows the zone, clicking opens a zoomed region view
// where EVERY spec slot in the zone is clickable — sprite or not.
const ZONES: [string, string[]][] = [
  ["head", ["head_base", "beanie", "headphones", "eyes", "eyebrows", "nose", "mouth", "beard"]],
  ["torso", ["torso", "pelvis", "left_upper_arm", "right_upper_arm", "left_forearm", "right_forearm", "left_hand", "right_hand"]],
  ["legs", ["left_leg", "right_leg", "left_boot", "right_boot"]],
];

type Box = { x: number; y: number; width: number; height: number };

function slotSpecBounds(slot: string): Box | null {
  return asm?.slots.find((s) => s.slot === slot)?.bounds ?? null;
}

function zoneBounds(name: string): Box | null {
  const zone = ZONES.find(([n]) => n === name);
  if (!zone) return null;
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const slot of zone[1]) {
    const b = slotSpecBounds(slot);
    if (!b) continue;
    x0 = Math.min(x0, b.x); y0 = Math.min(y0, b.y);
    x1 = Math.max(x1, b.x + b.width); y1 = Math.max(y1, b.y + b.height);
  }
  if (!isFinite(x0)) return null;
  return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
}

/** The body zone under a stage point. */
function partAt(x: number, y: number): string | null {
  for (const [name] of ZONES) {
    const b = zoneBounds(name);
    if (b && x >= b.x && x < b.x + b.width && y >= b.y && y < b.y + b.height) return name;
  }
  return null;
}

/** Zoomed region view: the zone blown up, every slot outlined and clickable. */
function openZoneView(zoneName: string) {
  if (!asm) return;
  const zone = ZONES.find(([n]) => n === zoneName);
  const zb = zoneBounds(zoneName);
  if (!zone || !zb) return;
  const PAD = 8;
  const vx = zb.x - PAD, vy = zb.y - PAD, vw = zb.width + PAD * 2, vh = zb.height + PAD * 2;
  const Z = Math.max(2, Math.floor(Math.min(560 / vw, 460 / vh))); // zoom factor

  detailEl.innerHTML = "";
  const back = el("button", { class: "btn ghost" }, ["← Builder"]);
  back.addEventListener("click", () => renderBuilder());

  const canvas = el("canvas", { style: "image-rendering:pixelated; border-radius:10px; box-shadow:0 8px 30px rgba(0,0,0,0.4)" });
  canvas.width = vw * Z;
  canvas.height = vh * Z;
  const ctx = canvas.getContext("2d")!;
  ctx.imageSmoothingEnabled = false;

  let zoneHover: string | null = null;
  const draw = () => {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    // The character, cropped to the zone.
    for (const slot of asm!.order) {
      const variant = sel.get(slot);
      if (!variant) continue;
      const inst = asm!.slots.find((s) => s.slot === slot)?.instances.find((v) => v.variant === variant);
      const img = imgCache.get(`${slot}/${variant}`);
      const cb = inst?.canonical_bounds;
      const r = inst?.region;
      if (!inst || !img || !cb || !r) continue;
      ctx.drawImage(img, r.x, r.y, r.w, r.h, (cb.x - vx) * Z, (cb.y - vy) * Z, cb.width * Z, cb.height * Z);
    }
    // Slot boxes: green = has sprites, gray = empty; hovered box glows.
    for (const slot of zone[1]) {
      const b = slotSpecBounds(slot);
      if (!b) continue;
      const has = (asm!.slots.find((s) => s.slot === slot)?.instances.length ?? 0) > 0;
      ctx.save();
      ctx.lineWidth = slot === zoneHover ? 3 : 1.5;
      ctx.strokeStyle = slot === zoneHover ? "#f2b45a" : has ? "rgba(87,196,100,0.75)" : "rgba(160,150,140,0.55)";
      if (slot !== zoneHover) ctx.setLineDash([3, 3]);
      ctx.strokeRect((b.x - vx) * Z, (b.y - vy) * Z, b.width * Z, b.height * Z);
      ctx.restore();
    }
    // Label for the hovered slot.
    if (zoneHover) {
      const b = slotSpecBounds(zoneHover)!;
      const label = ` ${zoneHover.replace(/_/g, " ")} `;
      ctx.font = "600 12px Consolas, monospace";
      const tw = ctx.measureText(label).width;
      const lx = Math.min((b.x - vx) * Z, canvas.width - tw - 6);
      const ly = Math.max(14, (b.y - vy) * Z - 5);
      ctx.fillStyle = "rgba(27,19,13,0.92)";
      ctx.fillRect(lx, ly - 12, tw + 4, 16);
      ctx.fillStyle = "#f2b45a";
      ctx.fillText(label, lx + 2, ly);
    }
  };

  const slotAtPoint = (ev: MouseEvent): string | null => {
    const rect = canvas.getBoundingClientRect();
    const x = ((ev.clientX - rect.left) / rect.width) * vw + vx;
    const y = ((ev.clientY - rect.top) / rect.height) * vh + vy;
    // Smallest matching box wins, so eyes are pickable on top of the head.
    let best: string | null = null, bestArea = Infinity;
    for (const slot of zone[1]) {
      const b = slotSpecBounds(slot);
      if (!b) continue;
      if (x >= b.x && x < b.x + b.width && y >= b.y && y < b.y + b.height) {
        const area = b.width * b.height;
        if (area < bestArea) { bestArea = area; best = slot; }
      }
    }
    return best;
  };
  canvas.addEventListener("mousemove", (ev) => {
    const s = slotAtPoint(ev);
    canvas.style.cursor = s ? "pointer" : "default";
    if (s !== zoneHover) { zoneHover = s; draw(); }
  });
  canvas.addEventListener("mouseleave", () => { zoneHover = null; draw(); });
  canvas.addEventListener("mousedown", (ev) => {
    const s = slotAtPoint(ev);
    if (s) void openPartEditor(s);
  });

  // Slot chips under the canvas — a clickable roster with coverage dots.
  const chips = el("div", { class: "row", style: "flex-wrap:wrap" });
  for (const slot of zone[1]) {
    const has = (asm.slots.find((s) => s.slot === slot)?.instances.length ?? 0) > 0;
    const chip = el("button", { class: "btn ghost", style: "padding:6px 10px; font-size:11px" }, [
      el("span", { class: "dot " + (has ? "has" : "miss") }),
      slot.replace(/_/g, " "),
    ]);
    chip.addEventListener("click", () => void openPartEditor(slot));
    chips.append(chip);
  }

  detailEl.append(
    el("div", { class: "card" }, [
      el("div", { class: "row", style: "margin-top:0" }, [back, el("h3", { style: "margin:0" }, [`${zoneName} — pick a part`])]),
      el("div", { class: "stage", style: "border:0; margin-top:10px" }, [canvas]),
      chips,
      el("div", { class: "hint" }, ["Green boxes have sprites; gray ones don't exist yet — click any box to analyze it and prompt the AI."]),
    ]),
  );
  draw();
}

// --- State generator: the WHOLE character in a new state --------------------
// The reliable pipeline: models keep his identity when drawing the full
// character, so we generate whole states and register them either sliced into
// the modular slots (head/torso/legs) or as a classic full-frame sprite.
const STATE_IDEAS: [string, string][] = [
  ["sitting_mug", "sitting cross-legged on a beanbag, both hands wrapped around a steaming coffee mug, relaxed half-lidded eyes"],
  ["sleeping", "asleep sitting upright, head tilted forward, eyes closed, tiny Zzz mood"],
  ["wave", "standing front view, right arm raised waving hello, warm smile"],
  ["dance", "mid-dance groove, arms up, one knee bent, eyes closed enjoying the music"],
  ["shocked", "standing front view, wide eyes, mouth open, hands slightly raised"],
  ["instant_transmission", "standing front view, two fingers of the right hand pressed to his forehead, calm focused expression (Dragon Ball instant transmission pose)"],
];

async function openStateView() {
  if (!asm) return;
  if (refSheet === null) refSheet = await referenceSheet().catch(() => null);
  if (!modelList.length) modelList = await imageModels().catch(() => []);

  detailEl.innerHTML = "";
  const back = el("button", { class: "btn ghost" }, ["← Builder"]);
  back.addEventListener("click", () => renderBuilder());

  const nameInput = el("input", { class: "field", style: "max-width:200px", placeholder: "state name (e.g. wave)" }) as HTMLInputElement;
  const modelSel = el("select", { class: "field", style: "max-width:300px" }) as HTMLSelectElement;
  for (const [id, label] of modelList) {
    const o = el("option", { value: id }, [label]) as HTMLOptionElement;
    if (id === PREFERRED_MODEL) o.selected = true;
    modelSel.append(o);
  }
  const promptBox = el("textarea", { class: "field", style: "min-height:110px" }) as HTMLTextAreaElement;
  const seed = (desc: string) =>
    "Using the attached master reference sheet as the ONLY visual identity, draw this EXACT character, full body, " +
    `${desc}. Same pixel-art style, same palette, same proportions and pixel density, hard 1px outlines, no anti-aliasing, ` +
    "single character centered on a plain dark background, no labels, no grid, no watermark.";
  promptBox.value = seed("standing front view, relaxed idle pose");

  // One-click state ideas (incl. the easter-egg poses).
  const ideas = el("div", { class: "row", style: "flex-wrap:wrap" });
  for (const [name, desc] of STATE_IDEAS) {
    const b = el("button", { class: "btn ghost", style: "padding:5px 9px; font-size:11px" }, [name.replace(/_/g, " ")]);
    b.addEventListener("click", () => {
      nameInput.value = name;
      promptBox.value = seed(desc);
    });
    ideas.append(b);
  }

  const genBtn = el("button", { class: "btn primary", id: "gen" }, ["Generate state"]);

  detailEl.append(
    el("div", { class: "card" }, [
      el("div", { class: "row", style: "margin-top:0" }, [back, el("h3", { style: "margin:0" }, ["New character state"])]),
      ideas,
      el("div", { class: "row" }, ["name", nameInput, "model", modelSel]),
      promptBox,
      el("div", { class: "row" }, [genBtn]),
      el("div", { class: "hint", style: "margin-top:8px" }, [
        "Whole-character generation keeps his identity. Accept it sliced into the modular slots, or as a classic full-frame sprite sheet entry — the same path the walk uses.",
      ]),
    ]),
    el("div", { class: "card", id: "sheetCard", style: "display:none" }, [
      el("h3", {}, ["Result"]),
      el("div", { id: "msg" }),
      el("div", { id: "sheetWrap" }),
    ]),
  );

  genBtn.addEventListener("click", async () => {
    const card = $("#sheetCard"), msg = $("#msg"), wrap = $("#sheetWrap");
    card.style.display = ""; wrap.innerHTML = ""; msg.innerHTML = "";
    msg.append(el("div", { class: "alert", html: `<span class="spin"></span> Generating with ${modelSel.value}…` }));
    genBtn.disabled = true;
    try {
      const res = await generateSprite(promptBox.value, "modular", null, 1, refSheet, modelSel.value);
      msg.innerHTML = "";
      const url = res.images[0];
      if (!url) { msg.append(el("div", { class: "alert err" }, ["No image returned."])); return; }
      wrap.append(el("div", { class: "thumb", style: "height:auto; max-height:340px; margin-bottom:10px" }, [el("img", { src: url })]));
      const asModular = el("button", { class: "btn primary" }, ["Accept → modular (slice head/torso/legs)"]) as HTMLButtonElement;
      const asClassic = el("button", { class: "btn" }, ["Accept → classic sprite"]) as HTMLButtonElement;
      const accept = async (mode: "modular" | "classic", btn: HTMLButtonElement) => {
        const name = nameInput.value.trim();
        if (!name) { msg.append(el("div", { class: "alert err" }, ["Give the state a name first."])); return; }
        btn.disabled = true; btn.textContent = "Saving…";
        try {
          const written = await stateAccept({ name, sheet: url, mode });
          msg.append(el("div", { class: "alert ok" }, [`Saved: ${written.join(", ")}`]));
          btn.textContent = "Accepted ✓";
          if (mode === "modular") asm = await assembly();
        } catch (e) {
          btn.disabled = false; btn.textContent = mode === "modular" ? "Accept → modular (slice head/torso/legs)" : "Accept → classic sprite";
          msg.append(el("div", { class: "alert err" }, [String(e)]));
        }
      };
      asModular.addEventListener("click", () => void accept("modular", asModular));
      asClassic.addEventListener("click", () => void accept("classic", asClassic));
      wrap.append(el("div", { class: "row" }, [asModular, asClassic]));
    } catch (e) {
      msg.innerHTML = "";
      msg.append(el("div", { class: "alert err" }, [String(e)]));
    } finally {
      genBtn.disabled = false;
    }
  });
}

// --- Parts-sheet generator: build him as parts, pick regions off the image --
async function openPartsSheet() {
  if (!asm) return;
  if (refSheet === null) refSheet = await referenceSheet().catch(() => null);
  if (!modelList.length) modelList = await imageModels().catch(() => []);

  detailEl.innerHTML = "";
  const back = el("button", { class: "btn ghost" }, ["← Builder"]);
  back.addEventListener("click", () => renderBuilder());

  const modelSel = el("select", { class: "field", style: "max-width:320px" }) as HTMLSelectElement;
  for (const [id, label] of modelList) {
    const o = el("option", { value: id }, [label]) as HTMLOptionElement;
    if (id === PREFERRED_MODEL) o.selected = true;
    modelSel.append(o);
  }

  const promptBox = el("textarea", { class: "field", style: "min-height:130px" }) as HTMLTextAreaElement;
  promptBox.value =
    "Using the attached master reference sheet as the ONLY visual identity, draw this EXACT character " +
    "DISASSEMBLED into isolated modular parts, laid out like a paper-doll / sprite-part sheet on a plain dark background: " +
    "head (no body), torso jacket (no head, no arms), left arm, right arm, left hand, right hand, " +
    "left leg, right leg, left boot, right boot — each part separated with clear empty space around it, " +
    "same pixel-art style, same palette, same pixel density, hard 1px outlines, no anti-aliasing, " +
    "no labels, no grid lines, no watermark. Every part must visually match the reference exactly.";

  const genBtn = el("button", { class: "btn primary", id: "gen" }, ["Generate parts sheet"]);

  detailEl.append(
    el("div", { class: "card" }, [
      el("div", { class: "row", style: "margin-top:0" }, [back, el("h3", { style: "margin:0" }, ["Parts sheet — generate, then pick parts off the image"])]),
      promptBox,
      el("div", { class: "row" }, [genBtn, el("span", { class: "count" }, ["model", modelSel])]),
      el("div", { class: "hint", style: "margin-top:8px" }, [
        "The master reference is attached automatically. After generating: drag a box around a part, choose which slot it is, and accept — it's keyed, fitted to the slot's canonical bounds, and registered.",
      ]),
    ]),
    el("div", { class: "card", id: "sheetCard", style: "display:none" }, [
      el("h3", {}, ["Pick a part"]),
      el("div", { id: "msg" }),
      el("div", { id: "sheetWrap" }),
    ]),
  );

  genBtn.addEventListener("click", async () => {
    const card = $("#sheetCard"), msg = $("#msg"), wrap = $("#sheetWrap");
    card.style.display = ""; wrap.innerHTML = ""; msg.innerHTML = "";
    msg.append(el("div", { class: "alert", html: `<span class="spin"></span> Generating with ${modelSel.value}…` }));
    genBtn.disabled = true;
    try {
      const res = await generateSprite(promptBox.value, "modular", null, 1, refSheet, modelSel.value);
      msg.innerHTML = "";
      const sheetUrl = res.images[0];
      if (!sheetUrl) { msg.append(el("div", { class: "alert err" }, ["No image returned."])); return; }
      buildRegionPicker(wrap, msg, sheetUrl);
    } catch (e) {
      msg.innerHTML = "";
      msg.append(el("div", { class: "alert err" }, [String(e)]));
    } finally {
      genBtn.disabled = false;
    }
  });
}

/** The marquee picker over a generated sheet: drag → crop preview → slot/variant → accept. */
function buildRegionPicker(wrap: HTMLElement, msg: HTMLElement, sheetUrl: string) {
  const img = new Image();
  img.onload = () => {
    const MAXW = 640;
    const view = Math.min(1, MAXW / img.width);
    const canvas = el("canvas", { style: "border-radius:10px; cursor:crosshair; max-width:100%" });
    canvas.width = Math.round(img.width * view);
    canvas.height = Math.round(img.height * view);
    const ctx = canvas.getContext("2d")!;
    let selRect: { x: number; y: number; w: number; h: number } | null = null;
    let start: [number, number] | null = null;

    const draw = () => {
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
      if (selRect) {
        ctx.save();
        ctx.strokeStyle = "#f2b45a";
        ctx.lineWidth = 2;
        ctx.setLineDash([5, 3]);
        ctx.strokeRect(selRect.x * view, selRect.y * view, selRect.w * view, selRect.h * view);
        ctx.restore();
      }
    };
    const toImg = (ev: MouseEvent): [number, number] => {
      const r = canvas.getBoundingClientRect();
      return [
        Math.round(((ev.clientX - r.left) / r.width) * img.width),
        Math.round(((ev.clientY - r.top) / r.height) * img.height),
      ];
    };
    canvas.addEventListener("mousedown", (ev) => { start = toImg(ev); selRect = null; draw(); });
    canvas.addEventListener("mousemove", (ev) => {
      if (!start) return;
      const [x, y] = toImg(ev);
      selRect = {
        x: Math.min(start[0], x), y: Math.min(start[1], y),
        w: Math.abs(x - start[0]), h: Math.abs(y - start[1]),
      };
      draw();
    });
    canvas.addEventListener("mouseup", () => { start = null; updateForm(); });

    // Assignment form: which slot/variant the selected region becomes.
    const slotSel = el("select", { class: "field", style: "max-width:200px" }) as HTMLSelectElement;
    for (const sp of asm?.slots ?? []) slotSel.append(el("option", { value: sp.slot }, [sp.slot.replace(/_/g, " ")]));
    const variantInput = el("input", { class: "field", style: "max-width:180px", value: "front", placeholder: "variant name" }) as HTMLInputElement;
    const preview = el("div", { class: "thumb", style: "width:110px; height:110px" });
    const acceptBtn = el("button", { class: "btn primary" }, ["Accept as part"]) as HTMLButtonElement;
    acceptBtn.disabled = true;

    const updateForm = () => {
      preview.innerHTML = "";
      if (!selRect || selRect.w < 4 || selRect.h < 4) { acceptBtn.disabled = true; return; }
      const c = document.createElement("canvas");
      c.width = selRect.w; c.height = selRect.h;
      c.getContext("2d")!.drawImage(img, selRect.x, selRect.y, selRect.w, selRect.h, 0, 0, selRect.w, selRect.h);
      preview.append(el("img", { src: c.toDataURL("image/png") }));
      acceptBtn.disabled = false;
    };
    acceptBtn.addEventListener("click", async () => {
      if (!selRect) return;
      acceptBtn.disabled = true; acceptBtn.textContent = "Saving…";
      try {
        const path = await regionAccept({
          slot: slotSel.value, variant: variantInput.value.trim() || "front",
          sheet: sheetUrl, x: selRect.x, y: selRect.y, w: selRect.w, h: selRect.h,
        });
        msg.append(el("div", { class: "alert ok" }, [`Saved ${slotSel.value}/${variantInput.value} → ${path}`]));
        asm = await assembly(); // the builder now knows the new part
        acceptBtn.textContent = "Accept as part";
        acceptBtn.disabled = false;
      } catch (e) {
        acceptBtn.disabled = false; acceptBtn.textContent = "Accept as part";
        msg.append(el("div", { class: "alert err" }, [String(e)]));
      }
    });

    wrap.append(
      canvas,
      el("div", { class: "row", style: "align-items:flex-start" }, [
        preview,
        el("div", {}, [
          el("div", { class: "row", style: "margin-top:0" }, ["slot", slotSel]),
          el("div", { class: "row" }, ["variant", variantInput]),
          el("div", { class: "row" }, [acceptBtn]),
        ]),
      ]),
      el("div", { class: "hint" }, ["Drag a box around one part on the sheet. It gets background-keyed, trimmed, fitted to the slot's canonical bounds and registered in the manifest. Repeat for each part."]),
    );
    draw();
  };
  img.src = sheetUrl;
}

/** Called from the Batches tab: preview a freshly-sliced cell on the live
 * character without saving. Adds an override for the slot and jumps to Builder. */
async function tryInBuilder(slot: string, variant: string, dataUrl: string) {
  try {
    const img = await loadImage(dataUrl);
    overrides.set(slot, { label: variant, img });
  } catch { /* ignore */ }
  // Switch the segmented control to Builder.
  const seg = $("#setSeg");
  seg.querySelectorAll("button").forEach((x) => x.classList.toggle("on", x.getAttribute("data-set") === "builder"));
  await loadMode("builder");
}

// --- selection + set switching ----------------------------------------------
function selectFlat(id: string) { selectedId = id; renderGrid(); renderFlatDetail(); }
function selectBatch(key: string) { selectedId = key; renderGrid(); renderBatchDetail(); }

async function loadMode(m: Mode) {
  mode = m; selectedId = null;
  // The left header + filter are only meaningful for the flat/batch lists.
  const lbl = document.getElementById("leftLbl");
  const searchWrap = document.getElementById("searchWrap");
  if (lbl) lbl.textContent = m === "builder" ? "Layers" : m === "batches" ? "Batches" : "Sprites";
  if (searchWrap) searchWrap.style.display = m === "builder" ? "none" : "";
  gridEl.innerHTML = "";
  gridEl.append(el("div", { class: "hint", style: "padding:10px" }, ["Loading…"]));
  try {
    if (m === "builder") {
      await prepareBuilder();
      renderBuilder();
      return;
    } else if (m === "batches") {
      if (!batchList.length) batchList = await batches();
      if (refSheet === null) refSheet = await referenceSheet();
    } else {
      sprites = await listSprites(m as SpriteSet);
    }
  } catch (e) {
    gridEl.innerHTML = "";
    gridEl.append(el("div", { class: "alert err", style: "margin:10px" }, [String(e)]));
    return;
  }
  renderGrid();
  if (m === "batches") renderBatchDetail();
  else renderFlatDetail();
}

function wireSetSeg() {
  const seg = $("#setSeg");
  seg.querySelectorAll("button").forEach((b) => {
    b.addEventListener("click", () => {
      seg.querySelectorAll("button").forEach((x) => x.classList.remove("on"));
      b.classList.add("on");
      loadMode((b.getAttribute("data-set") as Mode) || "classic");
    });
  });
}

async function boot() {
  if (!(await devMode())) {
    document.body.innerHTML =
      '<div style="display:grid;place-items:center;height:100vh;color:#888;font:14px sans-serif">Sprite Studio is developer-only. Launch with DESKFOLK_DEV=1.</div>';
    return;
  }
  ($("#search") as HTMLInputElement).addEventListener("input", (ev) => {
    filter = (ev.target as HTMLInputElement).value;
    renderGrid();
  });
  wireSetSeg();
  specs = await spriteSpecs().catch(() => null);
  await loadMode("classic");
}

boot();
