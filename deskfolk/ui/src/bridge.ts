// The typed seam between the web UI and Rust.
//
// Every `#[tauri::command]` the Rust side exposes has a thin wrapper here, so
// the rest of the UI never touches `invoke` directly and the whole contract is
// visible in one file. When we're running in a plain browser (`npm run dev`
// outside Tauri) there is no Rust to call, so each wrapper falls back to sane
// mock data — the wizard is fully clickable in a browser for fast iteration.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

// --- the shapes, mirroring the Rust serde types exactly ----------------------

export type Tier = "basic" | "advanced";
export type Mirror = "subtle" | "medium" | "strong";
export type BaselineMood = "chill" | "upbeat" | "deadpan";
export type NudgeLevel = "off" | "rare" | "gentle" | "naggy";
export type Autonomy = "homebody" | "roamer" | "free";
export type VoiceStyle = "low_easy" | "bright" | "gravel";

export type ProviderConfig =
  | { provider: "anthropic"; api_key: string; model: string }
  | { provider: "open-ai-compat"; base_url: string; api_key: string; model: string }
  | { provider: "sidecar"; base_url: string };

export interface QuietHours {
  start_hour: number;
  end_hour: number;
}

export interface Settings {
  first_run_complete: boolean;
  character: string;
  name: string | null;
  brain: { tier: Tier; provider: ProviderConfig | null };
  personality: {
    warmth: number;
    humor: number;
    edge: number;
    chattiness: number;
    curiosity: number;
    guardedness: number;
    mirror: Mirror;
    baseline_mood: BaselineMood;
    adult_register: boolean;
  };
  habits: {
    nudges: NudgeLevel;
    quiet_hours: QuietHours | null;
    autonomy: Autonomy;
    music_control: boolean;
  };
  voice: {
    enabled: boolean;
    style: VoiceStyle;
    wake_word: boolean;
    mic: string | null;
    speaker: string | null;
  };
  connect: { spotify: boolean; window_awareness: boolean };
}

export interface PackageSummary {
  id: string;
  name: string;
  tagline: string | null;
  author: string | null;
  dir: string;
  portrait: string | null;
}

export interface DeviceList {
  outputs: string[];
  inputs: string[];
  default_output: string | null;
  default_input: string | null;
  selected_output: string | null;
  selected_input: string | null;
}

// --- environment -------------------------------------------------------------

/** True inside a Tauri webview; false in a plain browser dev server. */
export const inTauri = "__TAURI_INTERNALS__" in window;

// --- the defaults the Rust side would also produce (mirrors Settings::default)

export function defaultSettings(): Settings {
  return {
    first_run_complete: false,
    character: "yasser",
    name: null,
    brain: { tier: "basic", provider: null },
    personality: {
      warmth: 70,
      humor: 80,
      edge: 45,
      chattiness: 55,
      curiosity: 65,
      guardedness: 30,
      mirror: "strong",
      baseline_mood: "chill",
      adult_register: false,
    },
    habits: { nudges: "gentle", quiet_hours: null, autonomy: "roamer", music_control: true },
    voice: { enabled: true, style: "low_easy", wake_word: true, mic: null, speaker: null },
    connect: { spotify: false, window_awareness: true },
  };
}

// --- command wrappers --------------------------------------------------------

export async function getSettings(): Promise<Settings> {
  if (!inTauri) return defaultSettings();
  return invoke<Settings>("get_settings");
}

export async function saveSettings(settings: Settings): Promise<void> {
  if (!inTauri) return;
  await invoke("save_settings", { settings });
}

export async function finishOnboarding(settings: Settings): Promise<void> {
  if (!inTauri) {
    console.info("[dev] finish_onboarding", settings);
    return;
  }
  await invoke("finish_onboarding", { settings });
}

export async function listCharacters(): Promise<PackageSummary[]> {
  if (!inTauri) {
    return [
      { id: "yasser", name: "Yasser", tagline: "Always here. Probably judging you.", author: null, dir: "yasser", portrait: null },
    ];
  }
  return invoke<PackageSummary[]>("list_characters");
}

export async function characterPortrait(dir: string): Promise<string | null> {
  if (!inTauri) return null;
  return invoke<string | null>("character_portrait", { dir });
}

export async function listAudioDevices(): Promise<DeviceList> {
  if (!inTauri) {
    return {
      outputs: ["Speakers (Realtek)"],
      inputs: ["Microphone (Realtek)", "Blue Yeti — USB"],
      default_output: "Speakers (Realtek)",
      default_input: "Microphone (Realtek)",
      selected_output: null,
      selected_input: null,
    };
  }
  return invoke<DeviceList>("list_audio_devices");
}

/** Close the current Tauri window (used by the wizard's ✕). */
export async function closeWindow(): Promise<void> {
  if (!inTauri) return;
  await getCurrentWindow().close();
}

// --- Sprite Studio (developer-only) -----------------------------------------

export type SpriteSet = "classic" | "modular";

export interface Specs {
  name: string;
  palette: string[];
  style_notes: string;
  character_notes: string;
  canvas: [number, number];
}

export interface SpriteEntry {
  id: string;
  label: string;
  rel_path: string;
  data_url: string;
  suggested_prompt: string;
  size: [number, number];
}

/** Is the dev surface enabled (DESKFOLK_DEV=1 or a debug build)? */
export async function devMode(): Promise<boolean> {
  if (!inTauri) return true;
  return invoke<boolean>("dev_mode");
}

export async function spriteSpecs(): Promise<Specs> {
  if (!inTauri) {
    return {
      name: "Yasser",
      palette: ["#FCE2A9", "#C6945D", "#7F5A35", "#2A1E14"],
      style_notes: "pixel art; hard 1px outline; no anti-aliasing; warm brown palette",
      character_notes: "stocky, full beard, beanie, headphones, brown puffer, dark jeans",
      canvas: [320, 320],
    };
  }
  return invoke<Specs>("sprite_specs");
}

export async function listSprites(set: SpriteSet): Promise<SpriteEntry[]> {
  if (!inTauri) {
    return [
      {
        id: "img_y_stand",
        label: "stand",
        rel_path: "sprites/img_y_stand.png",
        data_url: "",
        suggested_prompt: "Yasser standing, front view. pixel art…",
        size: [134, 292],
      },
    ];
  }
  return invoke<SpriteEntry[]>("list_sprites", { set });
}

export async function generateSprite(
  prompt: string,
  set: SpriteSet,
  refId: string | null,
  n: number,
  refUrl: string | null = null,
  model: string | null = null,
): Promise<{ images: string[] }> {
  if (!inTauri) {
    console.info("[dev] generate_sprite", { prompt, set, refId, n, model });
    return { images: [] };
  }
  // Rust params: prompt, set, ref_id, ref_url, n, model (camelCase auto-maps).
  return invoke<{ images: string[] }>("generate_sprite", { prompt, set, refId, refUrl, n, model });
}

// --- Batches (advanced: sheets of variant cells) ----------------------------

export interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface BatchCell {
  index: number;
  variant: string;
  label: string;
  has_sprite: boolean;
  data_url: string | null;
}

export interface Batch {
  slot: string;
  batch_id: string;
  title: string;
  cols: number;
  rows: number;
  cell_px: number;
  bounds: Bounds | null;
  pivot: number[] | null;
  prompt: string;
  cells: BatchCell[];
}

export async function batches(): Promise<Batch[]> {
  if (!inTauri) {
    return [
      {
        slot: "left_hand",
        batch_id: "batch_01",
        title: "Left hand batch 1",
        cols: 4,
        rows: 2,
        cell_px: 320,
        bounds: { x: 101, y: 198, width: 21, height: 25 },
        pivot: [111, 207],
        prompt: "Use the attached Yasser master reference…\n\n# Left hand batch 1\n…",
        cells: [
          { index: 0, variant: "relaxed", label: "Relaxed", has_sprite: false, data_url: null },
          { index: 1, variant: "fist", label: "Fist", has_sprite: false, data_url: null },
        ],
      },
    ];
  }
  return invoke<Batch[]>("batches");
}

export async function referenceSheet(): Promise<string | null> {
  if (!inTauri) return null;
  return invoke<string | null>("reference_sheet");
}

// --- Live builder assembly --------------------------------------------------

export interface PartInstance {
  variant: string;
  data_url: string;
  region: { x: number; y: number; w: number; h: number } | null;
  canonical_bounds: { x: number; y: number; width: number; height: number } | null;
}
export interface SlotParts {
  slot: string;
  bounds: { x: number; y: number; width: number; height: number } | null;
  pivot: number[] | null;
  instances: PartInstance[];
}
export interface Assembly {
  canvas: [number, number];
  order: string[];
  slots: SlotParts[];
}

export async function assembly(): Promise<Assembly> {
  if (!inTauri) {
    return {
      canvas: [320, 320],
      order: ["torso", "head_base"],
      slots: [
        { slot: "torso", bounds: null, pivot: null, instances: [] },
        { slot: "head_base", bounds: null, pivot: null, instances: [] },
      ],
    };
  }
  return invoke<Assembly>("assembly");
}

/** Accept a hand-picked rectangle of a generated parts sheet as a part. */
export async function regionAccept(req: {
  slot: string;
  variant: string;
  sheet: string;
  x: number;
  y: number;
  w: number;
  h: number;
}): Promise<string> {
  if (!inTauri) {
    console.info("[dev] region_accept", req);
    return "(dev) not written";
  }
  return invoke<string>("region_accept", { req });
}

/** Register a whole generated character state: sliced modular, or classic full-frame. */
export async function stateAccept(req: {
  name: string;
  sheet: string;
  mode: "modular" | "classic";
}): Promise<string[]> {
  if (!inTauri) {
    console.info("[dev] state_accept", req);
    return [];
  }
  return invoke<string[]>("state_accept", { req });
}

// --- Rig (skeleton + gestures) ----------------------------------------------

export interface Rig {
  joints: Record<string, [number, number]>;
  bones: [string, string][];
  attach: [string, string][];
  gestures: Record<string, Record<string, string>>;
}

/** Ultimate-pack prompt text for a slot (the default prompt source). */
export async function slotPrompt(slot: string): Promise<string | null> {
  if (!inTauri) return null;
  return invoke<string | null>("slot_prompt", { slot });
}

/** Current image-output models on OpenRouter: [id, label], newest first. */
export async function imageModels(): Promise<[string, string][]> {
  if (!inTauri) return [["google/gemini-3.1-flash-image", "Nano Banana 2 (default)"]];
  return invoke<[string, string][]>("image_models");
}

export async function rig(): Promise<Rig> {
  if (!inTauri) {
    return {
      joints: { root: [160, 290], chest: [160, 143], head: [160, 76] },
      bones: [["root", "chest"], ["chest", "head"]],
      attach: [["torso", "chest"], ["head_base", "head"]],
      gestures: {},
    };
  }
  return invoke<Rig>("rig");
}

/** Slice chosen cells out of a generated sheet and accept them as parts. */
export async function sliceAccept(req: {
  slot: string;
  sheet: string;
  cols: number;
  rows: number;
  cells: { index: number; variant: string }[];
  bounds: Bounds | null;
  pivot: number[] | null;
}): Promise<string[]> {
  if (!inTauri) {
    console.info("[dev] slice_accept", req);
    return [];
  }
  return invoke<string[]>("slice_accept", { req });
}

export async function acceptSprite(
  set: SpriteSet,
  id: string,
  dataUrl: string,
): Promise<string> {
  if (!inTauri) {
    console.info("[dev] accept_sprite", { set, id });
    return "(dev) not written";
  }
  // Rust param names: set, id, dataUrl -> keys must match: set, id, data_url? No —
  // Tauri converts snake_case params; the Rust fn param is `data_url`, so the JS
  // key must be `dataUrl` (Tauri v2 auto-maps camelCase to snake_case).
  return invoke<string>("accept_sprite", { set, id, dataUrl });
}
