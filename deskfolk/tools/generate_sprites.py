#!/usr/bin/env python3
"""Interactive sprite generator for the Deskfolk FLUX pack.

Walks the body-part prompt batches in `PROMPT_MANIFEST.md`, generates each
component sheet with FLUX (the master reference carries Yasser's identity and
style; the batch carries only the requested component, its grid and its
variants), shows you the result, and asks you to **keep it or regenerate** — with
an optional note to steer the next try ("thumb too long", "more olive in the
beanie"). It remembers what you've accepted, so you can stop and pick up later.

Two backends, same review loop:

  bfl     Black Forest Labs FLUX API — fully automatic. Needs an API key in
          the BFL_API_KEY environment variable. Uses FLUX.1 Kontext by default
          (image editing with the reference); pass --model for another.

  manual  No API, works anywhere: it copies the combined prompt to your
          clipboard and opens the reference image, you generate in FLUX
          yourself, then paste the path to the PNG you saved (drag it into the
          terminal). The accept/regenerate loop is identical.

Examples:
  python generate_sprites.py                         # auto backend (bfl if key, else manual)
  python generate_sprites.py --backend bfl
  python generate_sprites.py --only right_hand eyes  # just these parts
  python generate_sprites.py --pack "C:\\path\\to\\deskfolk_flux_pack"
"""
from __future__ import annotations

import argparse
import base64
import json
import math
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

# Where the pack usually lives on this machine, as a last-resort fallback so the
# tool "just works" even when run from elsewhere. --pack always wins.
FALLBACK_PACK = Path(
    r"C:\Users\yasse\Desktop\code\Tamagochi\deskfolk"
    r"\deskfolk_flux_generation_pack\deskfolk_flux_pack"
)


# --- locating the pack -------------------------------------------------------

def find_pack_root(explicit: str | None) -> Path:
    """The folder that contains prompts/GLOBAL_FLUX.md."""
    candidates: list[Path] = []
    if explicit:
        candidates.append(Path(explicit))
    here = Path(__file__).resolve()
    candidates += [here.parent.parent, Path.cwd(), FALLBACK_PACK]
    for c in candidates:
        if (c / "prompts" / "GLOBAL_FLUX.md").exists():
            return c
    sys.exit("Could not find the pack (no prompts/GLOBAL_FLUX.md). Pass --pack <dir>.")


@dataclass
class Batch:
    part: str          # e.g. "right_hand" or "props"
    ident: str         # e.g. "right_hand/batch_01" — unique key
    path: Path
    title: str


def load_batches(pack: Path) -> list[Batch]:
    """Every batch listed in the manifest, in order."""
    manifest = (pack / "PROMPT_MANIFEST.md").read_text(encoding="utf-8")
    out: list[Batch] = []
    for line in manifest.splitlines():
        m = re.search(r"`([^`]+\.md)`", line)
        if not m:
            continue
        rel = m.group(1)
        path = pack / rel
        if not path.exists():
            continue
        part = Path(rel).parts[-2]            # prompts/body_parts/<part>/batch_xx.md
        ident = f"{part}/{path.stem}"
        out.append(Batch(part=part, ident=ident, path=path,
                         title=first_heading(path) or ident))
    return out


def first_heading(path: Path) -> str | None:
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("#"):
            return line.lstrip("#").strip()
    return None


def final_size(batch_text: str) -> tuple[int, int]:
    m = re.search(r"Final size:\s*(\d+)\s*[x\u00d7]\s*(\d+)", batch_text)
    return (int(m.group(1)), int(m.group(2))) if m else (1280, 640)


def build_prompt(global_text: str, batch_text: str, notes: list[str]) -> str:
    prompt = global_text.strip() + "\n\n" + batch_text.strip()
    if notes:
        prompt += "\n\nAdjustments for this attempt:\n" + "\n".join(f"- {n}" for n in notes)
    return prompt


# --- progress (resume) -------------------------------------------------------

def load_progress(out: Path) -> dict:
    f = out / "progress.json"
    if f.exists():
        try:
            return json.loads(f.read_text(encoding="utf-8"))
        except Exception:
            pass
    return {"accepted": {}}


def save_progress(out: Path, progress: dict) -> None:
    (out / "progress.json").write_text(json.dumps(progress, indent=2), encoding="utf-8")


# --- backends ----------------------------------------------------------------

class GenError(RuntimeError):
    pass


def bfl_generate(prompt: str, ref: Path, out_path: Path, size: tuple[int, int],
                 model: str, api_key: str, seed: int | None) -> Path:
    """Generate with the Black Forest Labs FLUX API (async submit + poll)."""
    base = os.environ.get("BFL_API_BASE", "https://api.bfl.ai")
    endpoint = f"{base}/v1/{model}"
    w, h = size
    g = math.gcd(w, h) or 1
    payload = {
        "prompt": prompt,
        "input_image": base64.b64encode(ref.read_bytes()).decode(),
        "aspect_ratio": f"{w // g}:{h // g}",
        "output_format": "png",
        "prompt_upsampling": False,
        "safety_tolerance": 6,
    }
    if seed is not None:
        payload["seed"] = seed

    submit = _post_json(endpoint, payload, {"x-key": api_key})
    polling_url = submit.get("polling_url") or submit.get("result_url")
    if not polling_url:
        raise GenError(f"no polling_url in response: {submit}")

    deadline = time.time() + 180
    while time.time() < deadline:
        time.sleep(1.5)
        status = _get_json(polling_url, {"x-key": api_key})
        state = status.get("status")
        if state == "Ready":
            out_path.write_bytes(_get_bytes(status["result"]["sample"]))
            return out_path
        if state in ("Error", "Failed", "Content Moderated", "Request Moderated"):
            raise GenError(f"generation {state}: {status.get('result') or status}")
    raise GenError("timed out waiting for the image")


def manual_generate(prompt: str, ref: Path, out_path: Path, drop_dir: Path,
                    _size, _model, _key, _seed) -> Path:
    """No API: hand the prompt to the user, take back the PNG they made."""
    copy_to_clipboard(prompt)
    print("\n  Prompt copied to your clipboard. Opening the reference image...")
    open_file(ref)
    print("  In FLUX: attach that reference, paste the prompt (image-edit / multi-ref mode),")
    print("  generate, save the PNG, then drag it into this window and press Enter.")
    while True:
        raw = input("  Path to the generated PNG (or 'skip'): ").strip().strip('"').strip("'")
        if raw.lower() in ("skip", "s", ""):
            raise GenError("skipped")
        src = Path(raw)
        if src.is_file():
            out_path.write_bytes(src.read_bytes())
            return out_path
        dropped = sorted(drop_dir.glob("*.png"), key=lambda p: p.stat().st_mtime)
        if dropped:
            out_path.write_bytes(dropped[-1].read_bytes())
            return out_path
        print("  ...not found. Try again, or type 'skip'.")


def openrouter_generate(prompt: str, ref: Path, out_path: Path, _size,
                        model: str, api_key: str, seed: int | None) -> Path:
    """Generate with an image model on OpenRouter (e.g. Gemini image / nano
    banana) via the OpenAI-compatible chat endpoint: the reference goes in as an
    image, the prompt as text, and the model returns the edited component."""
    base = os.environ.get("OPENROUTER_API_BASE", "https://openrouter.ai/api/v1")
    ref_url = "data:image/png;base64," + base64.b64encode(ref.read_bytes()).decode()
    body = {
        "model": model,
        "modalities": ["image", "text"],
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": prompt},
                {"type": "image_url", "image_url": {"url": ref_url}},
            ],
        }],
    }
    if seed is not None:
        body["seed"] = seed
    headers = {
        "Authorization": f"Bearer {api_key}",
        "HTTP-Referer": "https://deskfolk.local",
        "X-Title": "Deskfolk sprite generator",
    }
    resp = _post_json(base + "/chat/completions", body, headers)
    if "error" in resp:
        raise GenError(str(resp["error"])[:300])
    msg = resp["choices"][0]["message"]
    # Image models return generated images in `message.images`.
    for img in msg.get("images") or []:
        url = (img.get("image_url") or {}).get("url") or img.get("url")
        if url and url.startswith("data:"):
            out_path.write_bytes(_data_url_bytes(url))
            return out_path
    # Some return an image content-part instead.
    content = msg.get("content")
    if isinstance(content, list):
        for part in content:
            u = (part.get("image_url") or {}).get("url", "") if isinstance(part, dict) else ""
            if u.startswith("data:"):
                out_path.write_bytes(_data_url_bytes(u))
                return out_path
    raise GenError(f"no image in reply (model said: {str(content)[:200]!r})")


def _data_url_bytes(url: str) -> bytes:
    return base64.b64decode(url.split(",", 1)[1])


def openrouter_key(pack: Path) -> str:
    """The key from the env, else from a nearby brain/.env — the same key the
    companion's mind already uses, so there is nothing extra to set."""
    k = os.environ.get("OPENROUTER_API_KEY", "").strip()
    if k:
        return k
    d = pack
    for _ in range(6):
        for cand in (d / "brain" / ".env", d / ".env"):
            if cand.exists():
                v = _env_value(cand, "OPENROUTER_API_KEY")
                if v:
                    return v
        d = d.parent
    return ""


def _env_value(path: Path, name: str) -> str:
    for line in path.read_text(encoding="utf-8", errors="ignore").splitlines():
        line = line.strip()
        if line.startswith("export "):
            line = line[7:]
        k, _, v = line.partition("=")
        if k.strip() == name:
            return v.split("#")[0].strip().strip('"').strip("'")
    return ""


BACKENDS = {"bfl": bfl_generate, "manual": manual_generate, "openrouter": openrouter_generate}


# --- tiny HTTP + OS helpers (stdlib only) ------------------------------------

def _post_json(url: str, body: dict, headers: dict) -> dict:
    req = urllib.request.Request(
        url, data=json.dumps(body).encode(), method="POST",
        headers={"Content-Type": "application/json", **headers})
    return _read_json(req)


def _get_json(url: str, headers: dict) -> dict:
    return _read_json(urllib.request.Request(url, headers=headers))


def _read_json(req) -> dict:
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        raise GenError(f"HTTP {e.code}: {e.read().decode()[:300]}") from e
    except urllib.error.URLError as e:
        raise GenError(f"network error: {e.reason}") from e


def _get_bytes(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=120) as r:
        return r.read()


def copy_to_clipboard(text: str) -> None:
    try:
        if sys.platform == "win32":
            subprocess.run("clip", input=text, text=True, check=True)
        elif sys.platform == "darwin":
            subprocess.run("pbcopy", input=text, text=True, check=True)
        else:
            subprocess.run(["xclip", "-selection", "clipboard"], input=text, text=True, check=True)
    except Exception:
        pass  # a nicety, not a requirement


def open_file(path: Path) -> None:
    try:
        if sys.platform == "win32":
            os.startfile(path)  # type: ignore[attr-defined]
        elif sys.platform == "darwin":
            subprocess.Popen(["open", str(path)])
        else:
            subprocess.Popen(["xdg-open", str(path)])
    except Exception:
        pass


def ask(prompt: str, allowed: str, default: str) -> str:
    while True:
        c = input(prompt).strip().lower()
        if c == "":
            return default
        if c[0] in allowed:
            return c[0]


# --- the interactive loop ----------------------------------------------------

def run(args) -> None:
    pack = find_pack_root(args.pack)
    global_text = (pack / "prompts" / "GLOBAL_FLUX.md").read_text(encoding="utf-8")
    ref = pack / "references" / "master_reference_sheet.png"
    if not ref.exists():
        sys.exit(f"missing reference image: {ref}")

    out = Path(args.out) if args.out else (pack / "generated")
    (out / "accepted").mkdir(parents=True, exist_ok=True)
    drop = out / "drop"
    drop.mkdir(exist_ok=True)
    progress = load_progress(out)

    batches = load_batches(pack)
    if args.only:
        wanted = set(args.only)
        batches = [b for b in batches if b.part in wanted or b.ident in wanted]

    or_key = openrouter_key(pack)
    bfl_key = os.environ.get("BFL_API_KEY", "").strip()

    backend = args.backend
    if backend == "auto":
        backend = "openrouter" if or_key else ("bfl" if bfl_key else "manual")

    if backend == "openrouter":
        api_key = or_key
        model = args.model or "google/gemini-2.5-flash-image"
        if not api_key:
            sys.exit("backend 'openrouter' needs OPENROUTER_API_KEY (env or a nearby brain/.env).")
    elif backend == "bfl":
        api_key = bfl_key
        model = args.model or "flux-kontext-pro"
        if not api_key:
            sys.exit("backend 'bfl' needs BFL_API_KEY set. Or use --backend manual.")
    else:  # manual
        api_key, model = "", args.model or ""

    todo = [b for b in batches if args.redo or b.ident not in progress["accepted"]]
    print(f"\nDeskfolk sprite generator — backend: {backend}")
    print(f"  pack: {pack}")
    print(f"  {len(todo)} of {len(batches)} batches to do "
          f"({len(progress['accepted'])} already accepted). Output: {out}\n")

    for i, b in enumerate(todo, 1):
        batch_text = b.path.read_text(encoding="utf-8")
        size = final_size(batch_text)
        notes: list[str] = []
        version = 0
        part_dir = out / b.part.replace("/", "_")
        part_dir.mkdir(exist_ok=True)
        print(f"[{i}/{len(todo)}] {b.title}  ({b.ident}, {size[0]}x{size[1]})")

        while True:
            version += 1
            prompt = build_prompt(global_text, batch_text, notes)
            img_path = part_dir / f"{b.path.stem}_v{version:02d}.png"
            seed = args.seed

            print(f"    generating v{version}...", end="", flush=True)
            try:
                gen = BACKENDS[backend](prompt, ref, img_path, size, model, api_key, seed)
                print(" done.")
            except GenError as e:
                print(f" {e}")
                choice = ask("    [r]etry  [s]kip  [q]uit: ", "rsq", "r")
                if choice == "q":
                    return finish(out, progress)
                if choice == "s":
                    break
                version -= 1  # a failure does not burn a version number
                continue

            open_file(gen)
            choice = ask("    keep it? [Enter]=yes  [r]egenerate  [s]kip  [p]rint prompt  [q]uit: ",
                         "yrspq", "y")
            if choice == "p":
                print("\n" + "-" * 70 + "\n" + prompt + "\n" + "-" * 70)
                version -= 1
                continue
            if choice == "q":
                return finish(out, progress)
            if choice == "s":
                print("    skipped.\n")
                break
            if choice == "r":
                note = input("    note to steer the next try (blank = just reroll): ").strip()
                if note:
                    notes.append(note)
                continue
            # accept
            dest = out / "accepted" / f"{b.part.replace('/', '_')}__{b.path.stem}.png"
            dest.write_bytes(gen.read_bytes())
            progress["accepted"][b.ident] = str(dest)
            save_progress(out, progress)
            print(f"    accepted -> {dest.name}\n")
            break

    finish(out, progress)


def finish(out: Path, progress: dict) -> None:
    save_progress(out, progress)
    print(f"\nDone. {len(progress['accepted'])} accepted; sheets are in {out / 'accepted'}.")
    print("Re-run any time to continue where you left off.")


def main() -> None:
    p = argparse.ArgumentParser(description="Interactive Deskfolk sprite generator.")
    p.add_argument("--pack", help="pack root (auto-detected otherwise)")
    p.add_argument("--out", help="output dir (default: <pack>/generated)")
    p.add_argument("--backend", choices=["auto", "openrouter", "bfl", "manual"], default="auto")
    p.add_argument("--model", default=None,
                   help="model id (default: google/gemini-2.5-flash-image on openrouter, "
                        "flux-kontext-pro on bfl)")
    p.add_argument("--only", nargs="*", help="only these parts/idents (e.g. right_hand eyes)")
    p.add_argument("--seed", type=int, help="fix the seed for reproducibility")
    p.add_argument("--redo", action="store_true", help="revisit already-accepted batches too")
    run(p.parse_args())


if __name__ == "__main__":
    main()
