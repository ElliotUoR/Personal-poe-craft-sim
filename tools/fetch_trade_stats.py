"""Fetch the pathofexile.com trade stat ids into data/trade_stats.json (spec §4.H).

    python tools/fetch_trade_stats.py

Source: https://www.pathofexile.com/api/trade/data/stats (public, no login). Only the groups the
link builder needs are kept: explicit, crafted, fractured, implicit, pseudo. Requests identify the
client only as "(ej)".
"""
import datetime as dt
import json
import os
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
URL = "https://www.pathofexile.com/api/trade/data/stats"
KEEP = {"explicit", "crafted", "fractured", "implicit", "pseudo"}


def main():
    req = urllib.request.Request(URL, headers={"User-Agent": "poecraft/0.1 (ej)"})
    with urllib.request.urlopen(req, timeout=60) as r:
        data = json.load(r)
    out = {"source": URL, "fetched": dt.date.today().isoformat(), "stats": []}
    for g in data["result"]:
        if g["id"] not in KEEP:
            continue
        for e in g["entries"]:
            out["stats"].append({"id": e["id"], "text": e["text"], "type": e.get("type", g["id"])})
    path = os.path.join(ROOT, "data", "trade_stats.json")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(out, f, ensure_ascii=False, separators=(",", ":"))
    print(f"wrote {path}: {len(out['stats'])} stats")


if __name__ == "__main__":
    main()
