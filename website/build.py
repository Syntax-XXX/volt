#!/usr/bin/env python3
"""Baut die Website aus README + docs/*.md nach website/dist/ (ohne Abhängigkeiten): python3 website/build.py"""
import html, re, pathlib
root = pathlib.Path(__file__).resolve().parent.parent
out = root / "website" / "dist"
out.mkdir(parents=True, exist_ok=True)
PAGES = [("index", "Start", "README.md"), ("tutorial", "Tutorial", "docs/TUTORIAL.md"), ("spec", "Sprache", "docs/SPEC.md"),
         ("entscheidungen", "Entscheidungen", "docs/DECISIONS.md"), ("benchmarks", "Benchmarks", "docs/BENCHMARKS.md")]
LINKS = {"SPEC.md": "spec.html", "DECISIONS.md": "entscheidungen.html", "TUTORIAL.md": "tutorial.html", "BENCHMARKS.md": "benchmarks.html",
         "docs/SPEC.md": "spec.html", "docs/DECISIONS.md": "entscheidungen.html", "docs/TUTORIAL.md": "tutorial.html", "docs/BENCHMARKS.md": "benchmarks.html"}

def inline(t):
    t = html.escape(t, quote=False)
    t = re.sub(r"`([^`]+)`", r"<code>\1</code>", t)
    t = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", t)
    t = re.sub(r"(?<![\w*])\*([^*\s][^*]*)\*(?!\w)", r"<em>\1</em>", t)
    return re.sub(r"\[([^\]]+)\]\(([^)]+)\)", lambda m: f'<a href="{LINKS.get(m[2], m[2])}">{m[1]}</a>', t)

def convert(md):
    lines, res, i = md.split("\n"), [], 0
    while i < len(lines):
        l = lines[i]
        if l.startswith("```"):
            lang, code = l[3:].strip(), []
            i += 1
            while i < len(lines) and not lines[i].startswith("```"):
                code.append(lines[i]); i += 1
            res.append(f'<pre><code class="lang-{lang}">{html.escape(chr(10).join(code))}</code></pre>')
        elif m := re.match(r"(#{1,4}) (.*)", l):
            n = len(m[1]); slug = re.sub(r"[^\w]+", "-", m[2].lower()).strip("-")
            res.append(f'<h{n} id="{slug}">{inline(m[2])}</h{n}>')
        elif l.startswith("|"):
            rows = []
            while i < len(lines) and lines[i].startswith("|"):
                if not re.match(r"^\|[\s:|-]+\|$", lines[i]):
                    rows.append([c.strip() for c in lines[i].strip("|").split("|")])
                i += 1
            i -= 1
            res.append("<table>" + "".join("<tr>" + "".join(f"<{'th' if k == 0 else 'td'}>{inline(c)}</{'th' if k == 0 else 'td'}>" for c in r) + "</tr>" for k, r in enumerate(rows)) + "</table>")
        elif re.match(r"\s*([-*]|\d+\.) ", l):
            tag = "ol" if re.match(r"\s*\d+\.", l) else "ul"
            items = []
            while i < len(lines) and re.match(r"\s*([-*]|\d+\.) ", lines[i]):
                items.append(re.sub(r"^\s*([-*]|\d+\.) ", "", lines[i])); i += 1
            i -= 1
            res.append(f"<{tag}>" + "".join(f"<li>{inline(x)}</li>" for x in items) + f"</{tag}>")
        elif l.strip():
            para = [l]
            while i + 1 < len(lines) and lines[i + 1].strip() and not re.match(r"```|#|\||\s*([-*]|\d+\.) ", lines[i + 1]):
                i += 1; para.append(lines[i])
            res.append(f"<p>{inline(' '.join(para))}</p>")
        i += 1
    return "\n".join(res)

CSS = """:root{--bg:#fbfaf7;--fg:#1d1b18;--mut:#6b655c;--acc:#d97a00;--code:#f1eee7;--line:#e3ded3}
@media(prefers-color-scheme:dark){:root{--bg:#14120f;--fg:#ece7dc;--mut:#9b9488;--acc:#ffb347;--code:#201d18;--line:#322e27}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:17px/1.65 Georgia,'Iowan Old Style',serif}
header{border-bottom:1px solid var(--line);position:sticky;top:0;background:var(--bg);z-index:2}
nav{max-width:860px;margin:auto;padding:12px 16px;display:flex;gap:18px;align-items:center;flex-wrap:wrap;font-family:system-ui,sans-serif;font-size:15px}
nav b{font-size:19px;margin-right:auto;color:var(--acc)}nav a{color:var(--mut);text-decoration:none}nav a.on,nav a:hover{color:var(--fg)}
main{max-width:860px;margin:auto;padding:8px 16px 80px}h1,h2,h3{font-family:system-ui,sans-serif;line-height:1.25;margin-top:2em}h1{font-size:2.2em;margin-top:1em}
a{color:var(--acc)}code{font:0.88em ui-monospace,Menlo,Consolas,monospace;background:var(--code);padding:.1em .35em;border-radius:4px}
pre{background:var(--code);border:1px solid var(--line);border-radius:8px;padding:14px 16px;overflow-x:auto;line-height:1.5}pre code{background:none;padding:0}
table{border-collapse:collapse;width:100%;font:15px system-ui,sans-serif;display:block;overflow-x:auto}td,th{border:1px solid var(--line);padding:6px 10px;text-align:left}th{background:var(--code)}
footer{text-align:center;color:var(--mut);font:14px system-ui,sans-serif;padding:30px}"""

for slug, title, src in PAGES:
    body = convert((root / src).read_text())
    nav = "".join(f'<a href="{s}.html" class="{"on" if s == slug else ""}">{t}</a>' for s, t, _ in PAGES)
    (out / f"{slug}.html").write_text(f'<!doctype html><html lang="de"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Volt – {title}</title><style>{CSS}</style></head><body><header><nav><b>⚡ Volt</b>{nav}</nav></header><main>{body}</main><footer>Volt – schnell wie C, lesbar wie Python</footer></body></html>')
print(f"{len(PAGES)} Seiten nach {out}")
