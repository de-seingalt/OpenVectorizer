"""Fetch benchmark images from Wikimedia Commons, keeping only public-domain / CC0 files.

  python3 -I scripts/fetch_commons.py probe "File:Foo.svg" ...      # show license/size, download nothing
  python3 -I scripts/fetch_commons.py fetch testdata/corpus/manifest.tsv testdata/corpus

Manifest (TSV, '#' comments): category <TAB> Commons title <TAB> size <TAB> note
  size: for SVG, the long side of the render in px; for raster, the width of the thumbnail requested.
Every downloaded file is listed with author, license and URL in <dest>/COMMONS.md (generated).
"""

import html
import json
import pathlib
import re
import sys
import time
import urllib.parse
import urllib.request

API = "https://commons.wikimedia.org/w/api.php"
USER_AGENT = "OpenVectorizer-corpus/0.1 (https://github.com/de-seingalt/OpenVectorizer)"
ALLOWED = re.compile(r"^(public domain|pd\b|pd-|cc0)", re.I)
MAX_SVG_BYTES = 1_500_000


def http_get(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    for attempt in range(4):
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return r.read()
        except Exception:  # noqa: BLE001 - retry on any network error
            if attempt == 3:
                raise
            time.sleep(2 * (attempt + 1))
    raise RuntimeError("unreachable")


def info(titles: list[str], thumb_width: int | None = None) -> dict:
    params = {
        "action": "query",
        "format": "json",
        "prop": "imageinfo",
        "iiprop": "url|size|mime|sha1|extmetadata",
        "iiextmetadatafilter": "LicenseShortName|Artist|UsageTerms",
        "titles": "|".join(titles),
    }
    if thumb_width:
        params["iiurlwidth"] = str(thumb_width)
    data = json.loads(http_get(API + "?" + urllib.parse.urlencode(params)))
    out = {}
    for page in data["query"]["pages"].values():
        title = page["title"]
        if "imageinfo" not in page:
            out[title] = None
            continue
        ii = page["imageinfo"][0]
        meta = ii.get("extmetadata", {})
        strip = lambda k: re.sub(r"<[^>]+>", "", html.unescape(meta.get(k, {}).get("value", ""))).strip()
        out[title] = {
            "url": ii["url"],
            "thumb": ii.get("thumburl"),
            "page": ii.get("descriptionurl"),
            "mime": ii["mime"],
            "bytes": ii["size"],
            "w": ii.get("width"),
            "h": ii.get("height"),
            "sha1": ii["sha1"],
            "license": strip("LicenseShortName"),
            "artist": strip("Artist")[:80] or "unknown",
        }
    # Normalized titles (spaces/underscores) map back by lowercase comparison.
    return out


def probe(titles: list[str]) -> None:
    for i in range(0, len(titles), 40):
        for title, d in info(titles[i : i + 40]).items():
            if d is None:
                print(f"MISSING  {title}")
                continue
            ok = "OK " if ALLOWED.match(d["license"]) else "NO "
            print(f"{ok} {d['license'][:22]:<22} {d['mime']:<14} {d['bytes']:>9} {d['w']}x{d['h']}  {title}")


def normalize_svg(text: str, long_side: int) -> str:
    """Set width/height on the root so the render size is fixed; derive a viewBox if missing."""
    m = re.search(r"<(?:svg:)?svg(?=[\s>])[^>]*>", text)
    if not m:
        raise ValueError("no <svg> root")
    tag = m.group(0)
    num = lambda name: (re.search(rf'\s{name}="\s*([\d.]+)\s*(px)?\s*"', tag) or [None, None])[1]
    vb = re.search(r'viewBox="([^"]+)"', tag)
    if vb:
        _, _, vw, vh = (float(v) for v in vb.group(1).replace(",", " ").split())
    else:
        w, h = num("width"), num("height")
        if not (w and h):
            raise ValueError("no viewBox and no unitless width/height")
        vw, vh = float(w), float(h)
        tag = re.sub(r"^<((?:svg:)?svg)", rf'<\1 viewBox="0 0 {vw:g} {vh:g}"', tag, count=1)
    k = long_side / max(vw, vh)
    W, H = max(1, round(vw * k)), max(1, round(vh * k))
    tag = re.sub(r'\s(width|height)="[^"]*"', "", tag)
    tag = re.sub(r"^<((?:svg:)?svg)", rf'<\1 width="{W}" height="{H}"', tag, count=1)
    return text[: m.start()] + tag + text[m.end() :]


def slug(title: str) -> str:
    stem = title.removeprefix("File:").rsplit(".", 1)[0]
    return re.sub(r"[^a-z0-9]+", "-", stem.lower()).strip("-")[:60]


def fetch(manifest: pathlib.Path, dest: pathlib.Path) -> None:
    rows = []
    for line in manifest.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        cat, title, size, *note = line.split("\t")
        rows.append((cat.strip(), title.strip(), int(size), note[0].strip() if note else ""))
    credits = []
    for cat, title, size, note in rows:
        d = info([title], thumb_width=size).get(title)
        if d is None:
            print(f"SKIP missing: {title}")
            continue
        if not ALLOWED.match(d["license"]):
            print(f"SKIP license '{d['license']}': {title}")
            continue
        out_dir = dest / cat
        out_dir.mkdir(parents=True, exist_ok=True)
        if d["mime"] == "image/svg+xml":
            if d["bytes"] > MAX_SVG_BYTES:
                print(f"SKIP too large ({d['bytes']} B): {title}")
                continue
            text = http_get(d["url"]).decode("utf-8")
            if "<text" in text:
                print(f"SKIP contains <text> (font-dependent render): {title}")
                continue
            try:
                text = normalize_svg(text, size)
            except ValueError as e:
                print(f"SKIP {e}: {title}")
                continue
            path = out_dir / f"{slug(title)}.svg"
            path.write_text(text, encoding="utf-8")
        else:
            data = http_get(d["thumb"] or d["url"])
            ext = {"image/png": "png", "image/jpeg": "jpg", "image/gif": "gif", "image/webp": "webp"}.get(
                d["mime"], "png"
            )
            if d["thumb"] and d["thumb"].endswith(".png"):
                ext = "png"
            path = out_dir / f"{slug(title)}.{ext}"
            path.write_bytes(data)
        credits.append((cat, path.name, title, d, note))
        print(f"OK   {cat}/{path.name}  [{d['license']}]")
        time.sleep(0.3)  # be gentle with the API
    lines = [
        "# Wikimedia Commons files (generated by scripts/fetch_commons.py)\n",
        "Only public-domain / CC0 files are accepted. Logos may be trademarks of their owners and are",
        "used here only as test data, never distributed with the application.\n",
        "| Fichier | Usage testé | Licence | Auteur | Source |",
        "|---|---|---|---|---|",
    ]
    for cat, name, title, d, note in credits:
        lines.append(f"| `{cat}/{name}` | {note} | {d['license']} | {d['artist'].replace('|', '/')} | [{title}]({d['page']}) |")
    (dest / "COMMONS.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    cmd, *args = sys.argv[1:]
    if cmd == "probe":
        probe(args)
    elif cmd == "fetch":
        fetch(pathlib.Path(args[0]), pathlib.Path(args[1]))
    else:
        sys.exit(__doc__)
