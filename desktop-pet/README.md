# Yasser Desktop Pet

The Watcher tamagotchi living on your PC screen. Same sprites, same brain
(`brain/think_server.py`), same behavior: click him to talk, he listens,
thinks, and talks back with his voice and mouth animation.

## Run

1. Start the brain (in `../brain`):

```powershell
cd ..\brain
python -m uvicorn think_server:app --host 0.0.0.0 --port 8087
```

2. Start the pet:

```powershell
pip install -r requirements.txt
python app.py
```

## Controls

- **Left click** — start listening (talk to him). Click again to send early
  (he auto-sends after ~1.4 s of silence). Click while he's speaking = shut up.
- **Drag** — move him around the screen.
- **Right click** — menu: type to him, toggle wake word / self-talk /
  always-on-top, force reactions, sleep, quit.
- **Say "Yasser"** — wake word is on by default; he perks up and listens.

## Config (env vars)

| var | default | meaning |
|-----|---------|---------|
| `PET_BRAIN` | `http://127.0.0.1:8087` | brain server URL |
| `PET_SCALE` | `1.3` | window size multiplier (412 px base) |

## Sprites

`assets/` is generated from the Watcher firmware's LVGL C arrays by
`convert_sprites.py`. To use new/custom sprites, drop same-named PNGs into
`assets/` (character sprites are bottom-center anchored). Re-run the
converter any time the firmware sprites change.
