// The first-run onboarding wizard — meeting Yasser.
//
// Seven steps, all editing one working copy of `Settings`; on finish it's
// persisted and the companion boots with it. Everything he "says" is set in
// mono, matching the design. Runs in a plain browser too (mock bridge) so the
// flow can be clicked through without launching the app.

import {
  type Settings,
  type PackageSummary,
  type DeviceList,
  type NudgeLevel,
  getSettings,
  finishOnboarding,
  listCharacters,
  characterPortrait,
  listAudioDevices,
  closeWindow,
} from "../bridge";

// --- tiny DOM helpers --------------------------------------------------------

type Attrs = Record<string, string | number | boolean | ((e: Event) => void)>;
function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...kids: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = String(v);
    else if (k === "html") node.innerHTML = String(v);
    else if (k.startsWith("on") && typeof v === "function")
      node.addEventListener(k.slice(2), v as EventListener);
    else if (v === true) node.setAttribute(k, "");
    else if (v !== false) node.setAttribute(k, String(v));
  }
  for (const kid of kids) node.append(kid);
  return node;
}

function say(text: string): HTMLElement {
  return el("div", { class: "say" }, text);
}

/** A labelled 0–100 slider row. */
function sliderRow(label: string, get: () => number, set: (v: number) => void): HTMLElement {
  const val = el("span", { class: "val" }, String(get()));
  const input = el("input", {
    type: "range",
    min: 0,
    max: 100,
    value: get(),
    oninput: (e) => {
      const v = Number((e.target as HTMLInputElement).value);
      set(v);
      val.textContent = String(v);
    },
  });
  return el(
    "div",
    { class: "row" },
    el("div", { class: "label" }, el("b", {}, label)),
    el("div", { class: "slider" }, input, val),
  );
}

/** A title/subtitle row with a toggle on the right. */
function toggleRow(
  title: string,
  sub: string,
  get: () => boolean,
  set: (v: boolean) => void,
): HTMLElement {
  const btn = el("button", { class: get() ? "toggle on" : "toggle", "aria-label": title });
  btn.addEventListener("click", () => {
    set(!get());
    btn.className = get() ? "toggle on" : "toggle";
  });
  const label = el("div", { class: "label" }, el("b", {}, title));
  if (sub) label.append(el("small", {}, sub));
  return el("div", { class: "row" }, label, btn);
}

/** A segmented control; returns the element and re-renders selection on click. */
function seg<T extends string>(
  options: [T, string][],
  get: () => T,
  set: (v: T) => void,
): HTMLElement {
  const wrap = el("div", { class: "seg" });
  const paint = () => {
    for (const b of Array.from(wrap.children) as HTMLElement[])
      b.className = b.dataset.v === get() ? "on" : "";
  };
  for (const [v, label] of options) {
    const b = el("button", { onclick: () => { set(v); paint(); } }, label);
    b.dataset.v = v;
    wrap.append(b);
  }
  paint();
  return wrap;
}

// --- state -------------------------------------------------------------------

let s: Settings;
let characters: PackageSummary[] = [];
let devices: DeviceList | null = null;
let idx = 0;

const app = document.getElementById("app")!;
const dots = document.getElementById("dots")!;
const backBtn = document.getElementById("back") as HTMLButtonElement;
const nextBtn = document.getElementById("next") as HTMLButtonElement;

// --- steps -------------------------------------------------------------------

interface Step {
  tag: string;
  next: string; // right-button label
  render(): HTMLElement;
}

const steps: Step[] = [
  // 1 · WELCOME -------------------------------------------------------------
  {
    tag: "Step 1 · Welcome",
    next: "Let's go",
    render() {
      const av = el("div", { class: "big-av" }, el("span", {}, "Y"));
      loadPortrait(s.character, av);
      return frag(
        el("div", { class: "center" },
          av,
          el("h1", {}, "Yo. I'm Yasser."),
          el("p", { class: "lede", style: "max-width:46ch;margin:0 auto" },
            "I live on your desktop now. I walk around, sit on your windows, judge your music taste. Give me two minutes and I'll set myself up around you — not the other way around."),
          say("Everything here can be changed later. Nothing leaves this machine without asking."),
        ),
      );
    },
  },
  // 2 · NAME & LOOK ---------------------------------------------------------
  {
    tag: "Step 2 · Name & look",
    next: "Next",
    render() {
      const chips = el("div", { class: "chips" });
      for (const c of characters) {
        const av = el("div", { class: "av" }, el("span", {}, c.name.charAt(0)));
        loadPortrait(c.dir, av);
        const chip = el("div", { class: c.dir === s.character ? "chip on" : "chip" }, av, el("span", { class: "nm" }, c.name));
        chip.addEventListener("click", () => { s.character = c.dir; render(); });
        chips.append(chip);
      }
      chips.append(el("div", { class: "chip soon" }, el("div", { class: "av" }, el("span", {}, "?")), el("span", { class: "nm" }, "More soon")));

      const name = el("input", { class: "field", value: displayName(), placeholder: "His name" }) as HTMLInputElement;
      const sayLine = () => `"${displayName()}. Solid choice. I'd have picked it myself. I did, actually."`;
      const saidEl = say(sayLine());
      name.addEventListener("input", () => {
        const v = name.value.trim();
        // Empty falls back to the package's own name.
        s.name = v && v !== characterName() ? v : null;
        saidEl.textContent = sayLine();
      });

      return frag(
        el("span", { class: "step-tag" }, "Name & look"),
        el("h2", {}, "What are you calling me?"),
        chips,
        el("div", { class: "lbl", style: "margin-top:6px" }, "His name"),
        name,
        el("p", { class: "muted", style: "margin:6px 0 0;font-size:12px" }, "This is also his wake word — say it out loud and he answers."),
        saidEl,
      );
    },
  },
  // 3 · PERSONALITY ---------------------------------------------------------
  {
    tag: "Step 3 · Personality & psychology",
    next: "Next",
    render() {
      const p = s.personality;
      const sliders = el("div", { class: "rows" },
        sliderRow("Warmth", () => p.warmth, (v) => (p.warmth = v)),
        sliderRow("Humor", () => p.humor, (v) => (p.humor = v)),
        sliderRow("Edge", () => p.edge, (v) => (p.edge = v)),
        sliderRow("Chattiness", () => p.chattiness, (v) => (p.chattiness = v)),
        sliderRow("Curiosity", () => p.curiosity, (v) => (p.curiosity = v)),
        sliderRow("Guardedness", () => p.guardedness, (v) => (p.guardedness = v)),
      );
      const mirror = el("div", { class: "card" },
        el("div", { class: "row" },
          el("div", { class: "label" }, el("b", {}, "Mirror strength"), el("small", {}, "How much your openness shapes him")),
          seg([["subtle", "Subtle"], ["medium", "Medium"], ["strong", "Strong"]], () => p.mirror, (v) => (p.mirror = v)),
        ),
      );
      const mood = el("div", { class: "row" },
        el("div", { class: "label" }, el("b", {}, "Baseline mood")),
        seg([["chill", "Chill"], ["upbeat", "Upbeat"], ["deadpan", "Deadpan"]], () => p.baseline_mood, (v) => (p.baseline_mood = v)),
      );
      const adult = toggleRow("Adult register (18+)", "Lets him swear and speak plainly. Off keeps it clean.", () => p.adult_register, (v) => (p.adult_register = v));

      return frag(
        el("span", { class: "step-tag" }, "Personality & psychology"),
        el("h2", {}, "Who do you want me to be?"),
        el("p", { class: "lede" }, "Rough defaults. I'll drift anyway — that's the point."),
        sliders,
        mirror,
        say("Open up and he opens up. Go cold and he gets short. Trust builds slow, breaks fast."),
        el("div", { class: "rows" }, mood, adult),
      );
    },
  },
  // 4 · HABITS --------------------------------------------------------------
  {
    tag: "Step 4 · Habits & autonomy",
    next: "Next",
    render() {
      const h = s.habits;
      const nudgeOn = () => h.nudges !== "off";
      const nudgeSeg = seg<NudgeLevel>(
        [["rare", "Rare"], ["gentle", "Gentle"], ["naggy", "Naggy"]],
        () => (h.nudges === "off" ? "gentle" : h.nudges),
        (v) => (h.nudges = v),
      );
      const nudges = el("div", { class: "card" },
        toggleRow("Proactive nudges", "Break, hydrate, stretch, posture — a word when you need it.", nudgeOn, (on) => { h.nudges = on ? "gentle" : "off"; render(); }),
      );
      if (nudgeOn()) nudges.append(el("div", { class: "row" }, el("div", { class: "label" }, el("small", {}, "How often")), nudgeSeg));

      // quiet hours
      const q = h.quiet_hours;
      const quiet = el("div", { class: "card" },
        toggleRow("Quiet hours", "He stays put and keeps quiet.", () => q !== null, (on) => { h.quiet_hours = on ? { start_hour: 23, end_hour: 8 } : null; render(); }),
      );
      if (q) {
        quiet.append(el("div", { class: "row" },
          el("div", { class: "label" }, el("small", {}, "From → to")),
          hourSelect(() => q.start_hour, (v) => (q.start_hour = v)),
          el("span", { class: "muted" }, "→"),
          hourSelect(() => q.end_hour, (v) => (q.end_hour = v)),
        ));
      }

      return frag(
        el("span", { class: "step-tag" }, "Habits & autonomy"),
        el("h2", {}, "How much should I meddle?"),
        el("p", { class: "lede" }, "I notice things. Hydration. Posture. Hour four of the same bug."),
        nudges,
        quiet,
        el("div", { class: "rows" },
          el("div", { class: "row" },
            el("div", { class: "label" }, el("b", {}, "Wandering"), el("small", {}, "Homebody stays put; free spirit walks, teleports, explores.")),
            seg([["homebody", "Homebody"], ["roamer", "Roamer"], ["free", "Free"]], () => h.autonomy, (v) => (h.autonomy = v)),
          ),
          toggleRow("Music control", "Play, pause, skip, volume. He has opinions.", () => h.music_control, (v) => (h.music_control = v)),
        ),
      );
    },
  },
  // 5 · VOICE & EARS --------------------------------------------------------
  {
    tag: "Step 5 · Voice & ears",
    next: "Next",
    render() {
      const v = s.voice;
      const voiceCard = el("div", { class: "card" },
        toggleRow("Voice", "He speaks out loud. Mute anytime from the tray.", () => v.enabled, (on) => { v.enabled = on; render(); }),
      );
      if (v.enabled) voiceCard.append(el("div", { class: "row" }, el("div", { class: "label" }, el("b", {}, "Sound")), seg([["low_easy", "Low & easy"], ["bright", "Bright"], ["gravel", "Gravel"]], () => v.style, (x) => (v.style = x))));

      // width:auto so the select sits beside its label instead of blowing the
      // row out to full width (.field is width:100%).
      const mic = el("select", { class: "field", style: "width:auto;max-width:58%" }) as HTMLSelectElement;
      const inputs = devices?.inputs ?? [];
      mic.append(el("option", { value: "" }, "System default"));
      for (const d of inputs) mic.append(el("option", { value: d, ...(v.mic === d ? { selected: true } : {}) }, d));
      mic.addEventListener("change", () => (v.mic = mic.value || null));

      return frag(
        el("span", { class: "step-tag" }, "Voice & ears"),
        el("h2", {}, "Can I talk? Can I listen?"),
        el("p", { class: "lede" }, "The wake word runs on this machine only. Nothing streams unless we're actually talking."),
        voiceCard,
        el("div", { class: "rows" },
          toggleRow("Wake word — \"" + displayName() + "\"", "Always-on, fully local. He perks up when you say his name.", () => v.wake_word, (x) => (v.wake_word = x)),
          el("div", { class: "row" }, el("div", { class: "label" }, el("b", {}, "Microphone"), el("small", {}, "Used only while you're talking to him.")), mic),
        ),
        say("Say something — \"…yeah, I can hear you.\""),
      );
    },
  },
  // 6 · CONNECT + BRAIN -----------------------------------------------------
  {
    tag: "Step 6 · Connect",
    next: "Next",
    render() {
      const c = s.connect;
      const brain = s.brain;
      const brainCard = el("div", { class: "card" },
        el("div", { class: "row" },
          el("div", { class: "label" }, el("b", {}, "The brain"), el("small", {}, "Basic is free, private, and runs on this machine. Advanced uses your own model.")),
          seg([["basic", "Built-in"], ["advanced", "My own"]], () => brain.tier, (v) => { brain.tier = v; if (v === "basic") brain.provider = null; render(); }),
        ),
      );
      if (brain.tier === "advanced") {
        const prov = brain.provider && brain.provider.provider === "open-ai-compat" ? brain.provider : { provider: "open-ai-compat" as const, base_url: "https://openrouter.ai/api/v1", api_key: "", model: "" };
        brain.provider = prov;
        const base = el("input", { class: "field", value: prov.base_url, placeholder: "Base URL (OpenAI-compatible)" }) as HTMLInputElement;
        const key = el("input", { class: "field", type: "password", value: prov.api_key, placeholder: "API key" }) as HTMLInputElement;
        const model = el("input", { class: "field", value: prov.model, placeholder: "Model id" }) as HTMLInputElement;
        base.addEventListener("input", () => (prov.base_url = base.value));
        key.addEventListener("input", () => (prov.api_key = key.value));
        model.addEventListener("input", () => (prov.model = model.value));
        brainCard.append(el("div", { class: "rows" },
          el("div", { class: "row" }, base), el("div", { class: "row" }, key), el("div", { class: "row" }, model)));
      }

      return frag(
        el("span", { class: "step-tag" }, "Connect"),
        el("h2", {}, "Optional hookups"),
        el("p", { class: "lede" }, "All skippable. I work fine without them, just… less impressively."),
        brainCard,
        el("div", { class: "rows" },
          toggleRow("♪ Spotify", "So he can DJ. Sees what's playing, nothing else.", () => c.spotify, (v) => (c.spotify = v)),
          toggleRow("▣ Window awareness", "Lets him sit on your windows and notice when you've been at one thing too long. Titles only, never content.", () => c.window_awareness, (v) => (c.window_awareness = v)),
        ),
        el("div", { class: "alert" },
          el("b", {}, "! If Windows blocks the mic"),
          say("\"Settings → Privacy → Microphone, flip it on for DeskFolk. Or don't — I'll read lips. I can't read lips.\"")),
      );
    },
  },
  // 7 · DONE ----------------------------------------------------------------
  {
    tag: "Step 7 · Done",
    next: "See you out there",
    render() {
      const av = el("div", { class: "big-av" }, el("span", {}, "Y"));
      loadPortrait(s.character, av);
      return el("div", { class: "center" },
        av,
        el("h1", {}, "That's it. I'm moving in."),
        el("p", { class: "lede", style: "max-width:46ch;margin:0 auto" }, "Say my name whenever. Click me if you're shy. Right-click me if you're mad. See you out there."),
      );
    },
  },
];

// --- helpers -----------------------------------------------------------------

function frag(...kids: Node[]): HTMLElement {
  const d = el("div");
  for (const k of kids) d.append(k);
  return d;
}

function hourSelect(get: () => number, set: (v: number) => void): HTMLSelectElement {
  const sel = el("select", { class: "field", style: "width:auto" }) as HTMLSelectElement;
  for (let h = 0; h < 24; h++) {
    const label = `${String(h).padStart(2, "0")}:00`;
    sel.append(el("option", { value: h, ...(get() === h ? { selected: true } : {}) }, label));
  }
  sel.addEventListener("change", () => set(Number(sel.value)));
  return sel;
}

/** The package's own name for the selected character. */
function characterName(): string {
  return characters.find((x) => x.dir === s.character)?.name ?? "Yasser";
}

/** What to call him: the user's override if any, else the package name. */
function displayName(): string {
  return s.name?.trim() || characterName();
}

const portraitCache = new Map<string, string>();
async function loadPortrait(dir: string, holder: HTMLElement): Promise<void> {
  let url = portraitCache.get(dir);
  if (url === undefined) {
    url = (await characterPortrait(dir)) ?? "";
    portraitCache.set(dir, url);
  }
  if (url) {
    holder.innerHTML = "";
    holder.append(el("img", { src: url, alt: "" }));
  }
}

// --- controller --------------------------------------------------------------

function render(): void {
  const step = steps[idx]!;
  app.innerHTML = "";
  app.append(step.render());
  app.scrollTop = 0;

  // dots
  dots.innerHTML = "";
  for (let i = 0; i < steps.length; i++) dots.append(el("div", { class: i === idx ? "dot on" : "dot" }));

  // footer
  backBtn.textContent = idx === 0 ? "Skip the tour" : "Back";
  nextBtn.textContent = step.next;
}

async function goNext(): Promise<void> {
  if (idx < steps.length - 1) {
    idx++;
    render();
  } else {
    await finish();
  }
}

function goBack(): void {
  if (idx === 0) {
    // "Skip the tour" — accept the defaults and get out of his way.
    void finish();
  } else {
    idx--;
    render();
  }
}

async function finish(): Promise<void> {
  nextBtn.disabled = true;
  try {
    await finishOnboarding(s);
  } catch (e) {
    // Even if booting hiccups, the wizard should still get out of the way.
    console.error("finish_onboarding failed", e);
  }
  await closeWindow();
}

async function boot(): Promise<void> {
  s = await getSettings();
  [characters, devices] = await Promise.all([listCharacters(), listAudioDevices()]);
  if (!characters.some((c) => c.dir === s.character) && characters[0]) s.character = characters[0].dir;

  backBtn.addEventListener("click", goBack);
  nextBtn.addEventListener("click", () => void goNext());
  document.getElementById("close")!.addEventListener("click", () => void closeWindow());

  render();
}

void boot();
