#!/usr/bin/env python3
"""
yasserctl — drive and debug the Watcher from your laptop.

    export YASSER_IP=192.168.1.62          # or pass --ip

    yasserctl.py info                      # version, reset, heap, wifi, screen
    yasserctl.py log                       # pet log (incl. pre-crash trail)
    yasserctl.py follow                    # live-tail the log (run during a crash!)
    yasserctl.py shot                      # screenshot -> shots/HH-MM-SS.png
    yasserctl.py say "hello from the CLI"
    yasserctl.py react suspicious          # any of the 19 emotions
    yasserctl.py nav menu|camera|listen|settings|home
    yasserctl.py brain [url]               # show or set the mind's address
    yasserctl.py reboot
    yasserctl.py watch                     # screenshot+log every 3s until Ctrl+C
"""
import os, sys, time, urllib.request, urllib.parse, pathlib

IP = os.environ.get("YASSER_IP", "192.168.1.62")
args = sys.argv[1:]
if args and args[0] == "--ip":
    IP = args[1]; args = args[2:]

def get(path, timeout=6):
    with urllib.request.urlopen(f"http://{IP}{path}", timeout=timeout) as r:
        return r.read()

def text(path):
    print(get(path).decode(errors="replace").rstrip())

def shot(tag=""):
    pathlib.Path("shots").mkdir(exist_ok=True)
    raw = get("/shot.bmp", timeout=10)
    ts = time.strftime("%H-%M-%S") + tag
    bmp = f"shots/{ts}.bmp"
    open(bmp, "wb").write(raw)
    try:
        from PIL import Image
        png = f"shots/{ts}.png"
        Image.open(bmp).save(png)
        os.remove(bmp)
        print(png)
    except Exception:
        print(bmp, "(pip install pillow for png)")

def follow():
    print(f"tailing {IP} — Ctrl+C to stop")
    last = ""
    while True:
        try:
            cur = get("/log").decode(errors="replace")
            if cur != last:
                new = cur[len(last):] if cur.startswith(last) else cur
                sys.stdout.write(new)
                sys.stdout.flush()
                last = cur
        except Exception as e:
            print(f"\n[watcher unreachable: {e} — reboot? retrying]")
            last = ""
            time.sleep(2)
        time.sleep(1)

cmd = args[0] if args else "info"
arg = " ".join(args[1:])

if   cmd == "info":   text("/info")
elif cmd == "log":    text("/log")
elif cmd == "follow": follow()
elif cmd == "shot":   shot()
elif cmd == "say":    text("/ctl?cmd=say&arg=" + urllib.parse.quote(arg))
elif cmd == "react":  text("/ctl?cmd=react&arg=" + urllib.parse.quote(arg))
elif cmd == "nav":    text("/ctl?cmd=nav&arg=" + urllib.parse.quote(arg))
elif cmd == "reboot": text("/ctl?cmd=reboot")
elif cmd == "brain":  text("/brain" + (f"?url={urllib.parse.quote(arg)}" if arg else ""))
elif cmd == "watch":
    while True:
        try:
            shot()
            text("/info")
        except Exception as e:
            print("unreachable:", e)
        time.sleep(3)
else:
    print(__doc__)
