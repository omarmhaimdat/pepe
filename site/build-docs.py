#!/usr/bin/env python3
"""Build the documentation site: docs/*.md -> site/docs/*.html.

Plain Python, no packages: the Markdown pepe's docs use is small (headings,
paragraphs, fenced code, inline code, bold, links, images, tables, lists,
quotes, and two small blocks of its own: `:::cards` and `:::note`), and a
converter for that is shorter than a dependency. Every page gets the same
frame (site/docs/docs.css, docs.js): a top bar with search and a theme
switch, a grouped sidebar, an "on this page" column, copy buttons on code,
and previous/next links. A search index is written beside the pages, and
a sitemap at the site's root.

    python3 site/build-docs.py           # write site/docs/
    python3 site/build-docs.py --check   # exit 1 when site/docs/ is stale
"""

import html
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCE = os.path.join(ROOT, "docs")
OUT = os.path.join(ROOT, "site", "docs")
SITE = "https://pepe.mhaimdat.com/docs/"
REPO = "https://github.com/omarmhaimdat/pepe"

# The sidebar, in groups and in order: (file stem, title). A page's own
# heading is its title; this is what the sidebar calls it.
GROUPS = [
    ("Start here", [("index", "Overview"), ("install", "Install")]),
    ("Modes", [
        ("load-test", "Load testing"),
        ("dashboard", "The dashboard"),
        ("ping", "Ping"),
        ("ramp", "Ramp"),
        ("api", "OpenAPI"),
        ("flow", "Flows"),
        ("replay", "Replay"),
        ("logs", "nginx logs"),
        ("compare", "Compare"),
    ]),
    ("Integrate", [
        ("output", "Output and exit codes"),
        ("ci", "CI and Docker"),
        ("agents", "Agents and scripts"),
    ]),
    ("Reference", [("reference", "Command reference"), ("benchmarks", "Benchmarks")]),
]
PAGES = [page for _, pages in GROUPS for page in pages]

# What the browser tab and a search result say, with the words someone
# would search for; the sidebar keeps the short name above.
TITLES = {
    "index": "pepe documentation · HTTP load testing and latency tools",
    "install": "Install pepe · macOS, Linux, Windows, Homebrew, Docker, Nix",
    "load-test": "Load testing an HTTP endpoint with pepe",
    "dashboard": "The live load-test dashboard · pepe",
    "ping": "pepe ping · HTTP ping with latency split by phase",
    "ramp": "pepe ramp · find an API's capacity step by step",
    "api": "pepe api · load-test every endpoint of an OpenAPI spec",
    "flow": "pepe flow · chained requests, each step fed by the last",
    "replay": "pepe replay · an access log replayed as real traffic",
    "logs": "pepe logs · a live dashboard for nginx access and error logs",
    "compare": "pepe compare · what changed between two load tests",
    "output": "Output, JSON report and exit codes · pepe",
    "ci": "pepe in CI · GitHub Action, Docker and --fail-if thresholds",
    "agents": "pepe for agents and scripts · MCP server and guardrails",
    "reference": "pepe command reference · every flag and subcommand",
    "benchmarks": "Benchmarks · pepe against wrk, oha, vegeta and k6",
}
HOME = "https://pepe.mhaimdat.com/"
OG_IMAGE = HOME + "img/og.png"
AUTHOR = {"@type": "Person", "name": "Omar Mhaimdat", "url": "https://github.com/omarmhaimdat"}

INLINE_CODE = re.compile(r"`([^`]+)`")
BOLD = re.compile(r"\*\*(.+?)\*\*")
EM = re.compile(r"(?<![\w*])\*(?!\*)([^*\n]+?)\*(?![\w*])")
LINK = re.compile(r"\[([^\]]+)\]\(([^)\s]+)\)")
IMAGE = re.compile(r"!\[([^\]]*)\]\(([^)\s]+)\)")


def slug(text):
    text = re.sub(r"`|\*", "", text).lower()
    text = re.sub(r"[^a-z0-9]+", "-", text).strip("-")
    return text or "section"


def href(url):
    """Links between docs pages are written to the .md and served as .html;
    links into the repository stay links into the repository."""
    if re.match(r"^[a-z0-9-]+\.md(#.*)?$", url):
        return url.replace(".md", ".html", 1)
    if url.startswith(("http://", "https://", "#", "mailto:")):
        return url
    return REPO + "/blob/master/" + url.lstrip("./")


def asset(url):
    """Images are the repository's assets/, which the site mirrors."""
    if url.startswith(("http://", "https://")):
        return url
    return "../" + url.lstrip("./")


def inline(text):
    """Inline markup to HTML. Code spans are taken out first, so what is
    inside them is never read as markup."""
    spans = []

    def keep(match):
        spans.append("<code>%s</code>" % html.escape(match.group(1)))
        return "\0%d\0" % (len(spans) - 1)

    text = INLINE_CODE.sub(keep, text)
    text = html.escape(text, quote=False)
    text = IMAGE.sub(lambda m: '<img src="%s" alt="%s" loading="lazy">' % (asset(m.group(2)), m.group(1)), text)
    text = LINK.sub(lambda m: '<a href="%s">%s</a>' % (href(m.group(2)), m.group(1)), text)
    text = BOLD.sub(r"<strong>\1</strong>", text)
    text = EM.sub(r"<em>\1</em>", text)
    return re.sub(r"\0(\d+)\0", lambda m: spans[int(m.group(1))], text)


def plain(text):
    """Text without markup, for the search index and descriptions."""
    text = IMAGE.sub("", text)
    text = LINK.sub(r"\1", text)
    return re.sub(r"[`*]", "", text).strip()


def table(rows):
    cells = [[c.strip() for c in r.strip().strip("|").split("|")] for r in rows]
    head, body = cells[0], cells[2:]
    out = ['<div class="table"><table><thead><tr>']
    out += ["<th>%s</th>" % inline(c) for c in head]
    out.append("</tr></thead><tbody>")
    for row in body:
        out.append("<tr>" + "".join("<td>%s</td>" % inline(c) for c in row) + "</tr>")
    out.append("</tbody></table></div>")
    return "".join(out)


def cards(lines):
    """`- [Title](page.md) — what it is`, one card each"""
    out = ['<div class="cards">']
    for line in lines:
        m = re.match(r"^-\s+\[([^\]]+)\]\(([^)]+)\)\s*(?:—|-)\s*(.*)$", line.strip())
        if not m:
            continue
        out.append('<a class="card" href="%s"><strong>%s</strong><span>%s</span></a>'
                   % (href(m.group(2)), html.escape(m.group(1)), inline(m.group(3))))
    out.append("</div>")
    return "".join(out)


def hero(lines):
    """The overview's opening: a logo line, a heading, a sentence, and a
    line of links that become the buttons."""
    out = ['<div class="hero">']
    for line in lines:
        line = line.strip()
        if not line:
            continue
        m = IMAGE.fullmatch(line)
        if m:
            out.append('<img src="%s" alt="%s" width="96" height="84">' % (asset(m.group(2)), html.escape(m.group(1), quote=True)))
            continue
        m = re.match(r"^#\s+(.*)$", line)
        if m:
            out.append("<h1>%s</h1>" % inline(m.group(1)))
            continue
        if LINK.sub("", line).strip() == "":
            out.append('<p class="buttons">%s</p>' % inline(line))
            continue
        out.append("<p>%s</p>" % inline(line))
    out.append("</div>")
    return "".join(out)


def convert(text):
    """Markdown to HTML, the headings for the page's own list, and the
    sections for the search index."""
    lines = text.split("\n")
    out, headings, sections = [], [], []
    current = {"title": "", "anchor": "", "text": []}
    i = 0
    paragraph = []

    def flush():
        if paragraph:
            out.append("<p>%s</p>" % inline(" ".join(paragraph)))
            current["text"].append(plain(" ".join(paragraph)))
            paragraph.clear()

    def close_section():
        if current["title"] or current["text"]:
            sections.append({"t": current["title"], "a": current["anchor"], "x": " ".join(current["text"])[:400]})

    while i < len(lines):
        line = lines[i]
        stripped = line.strip()
        if stripped.startswith(":::"):
            flush()
            kind = stripped[3:].strip() or "note"
            i += 1
            block = []
            while i < len(lines) and lines[i].strip() != ":::":
                block.append(lines[i])
                i += 1
            i += 1
            if kind == "cards":
                out.append(cards(block))
            elif kind == "hero":
                out.append(hero(block))
                current["text"].append(plain(" ".join(l for l in block if l.strip() and not l.strip().startswith(("#", "!", "[")))))
            else:
                inner, _, _ = convert("\n".join(block))
                out.append('<div class="callout %s">%s</div>' % (html.escape(kind), inner))
            continue
        if stripped.startswith("```"):
            flush()
            lang = stripped[3:].strip()
            i += 1
            code = []
            while i < len(lines) and not lines[i].strip().startswith("```"):
                code.append(lines[i])
                i += 1
            i += 1
            label = html.escape(lang) if lang else ""
            out.append('<figure class="code"><div class="bar"><span>%s</span><button type="button" class="copy" aria-label="Copy">copy</button></div>'
                       '<pre><code>%s</code></pre></figure>' % (label, html.escape("\n".join(code))))
            current["text"].append(" ".join(code)[:200])
            continue
        heading = re.match(r"^(#{1,4})\s+(.*)$", stripped)
        if heading:
            flush()
            level = len(heading.group(1))
            title = heading.group(2).strip()
            anchor = slug(title)
            if level == 1:
                out.append("<h1>%s</h1>" % inline(title))
            else:
                close_section()
                current = {"title": plain(title), "anchor": anchor, "text": []}
                headings.append((level, plain(title), anchor))
                out.append('<h%d id="%s"><a href="#%s">%s</a></h%d>' % (level, anchor, anchor, inline(title), level))
            i += 1
            continue
        if stripped.startswith("|"):
            flush()
            rows = []
            while i < len(lines) and lines[i].strip().startswith("|"):
                rows.append(lines[i])
                i += 1
            out.append(table(rows))
            current["text"].append(plain(" ".join(r.replace("|", " ") for r in rows[2:]))[:300])
            continue
        if re.match(r"^[-*]\s+", stripped) or re.match(r"^\d+\.\s+", stripped):
            flush()
            ordered = bool(re.match(r"^\d+\.", stripped))
            tag = "ol" if ordered else "ul"
            items = []
            while i < len(lines):
                item = lines[i].strip()
                if ordered and re.match(r"^\d+\.\s+", item):
                    items.append(re.sub(r"^\d+\.\s+", "", item))
                elif not ordered and re.match(r"^[-*]\s+", item):
                    items.append(re.sub(r"^[-*]\s+", "", item))
                elif item and lines[i].startswith("  ") and items:
                    items[-1] += " " + item
                else:
                    break
                i += 1
            out.append("<%s>%s</%s>" % (tag, "".join("<li>%s</li>" % inline(x) for x in items), tag))
            current["text"].append(plain(" ".join(items))[:300])
            continue
        if stripped.startswith(">"):
            flush()
            quote = []
            while i < len(lines) and lines[i].strip().startswith(">"):
                quote.append(lines[i].strip()[1:].strip())
                i += 1
            inner, _, _ = convert("\n".join(quote))
            out.append("<blockquote>%s</blockquote>" % inner)
            continue
        if not stripped:
            flush()
            i += 1
            continue
        paragraph.append(stripped)
        i += 1
    flush()
    close_section()
    return "\n".join(out), headings, sections


def first_paragraph(text):
    m = re.search(r"^:::hero\n(.*?)^:::$", text, re.S | re.M)
    if m:
        for line in m.group(1).split("\n"):
            line = line.strip()
            if line and not line.startswith(("#", "!")) and LINK.sub("", line).strip():
                return plain(line)
    for block in text.split("\n\n"):
        block = block.strip()
        if block and not block.startswith(("#", "```", "|", "-", "!", ">", ":::")):
            return plain(block).replace("\n", " ")
    return ""


def description(text, limit=160):
    """The page's opening, cut at the end of a sentence to about what a
    search result shows, never in the middle of a word."""
    paragraph = re.sub(r"\s+", " ", first_paragraph(text)).strip()
    out = ""
    for sentence in re.split(r"(?<=[.!?])\s+", paragraph):
        if out and len(out) + 1 + len(sentence) > limit:
            # A short opening says too little on its own: take the start
            # of the next sentence too
            if len(out) < limit // 2:
                out = out + " " + sentence
            break
        out = (out + " " + sentence).strip()
    if len(out) > limit:
        out = out[:limit - 1].rsplit(" ", 1)[0].rstrip(",;:") + "…"
    return out


def sidebar(stem):
    out = []
    for group, pages in GROUPS:
        out.append('<div class="group">%s</div>' % html.escape(group))
        for s, t in pages:
            out.append('<a href="%s.html"%s>%s</a>' % (s, ' aria-current="page"' if s == stem else "", html.escape(t)))
    return "".join(out)


def page(stem, title, body, headings, description):
    index = [s for s, _ in PAGES].index(stem)
    prev = PAGES[index - 1] if index > 0 else None
    nxt = PAGES[index + 1] if index + 1 < len(PAGES) else None
    turn = '<nav class="turn">'
    turn += ('<a class="prev" href="%s.html"><small>Previous</small>%s</a>' % (prev[0], html.escape(prev[1]))) if prev else "<span></span>"
    turn += ('<a class="next" href="%s.html"><small>Next</small>%s</a>' % (nxt[0], html.escape(nxt[1]))) if nxt else "<span></span>"
    turn += "</nav>"
    toc = "".join(
        '<a href="#%s" class="h%d">%s</a>' % (anchor, level, html.escape(t))
        for level, t, anchor in headings
        if level <= 3
    )
    canonical = SITE if stem == "index" else SITE + stem + ".html"
    full_title = TITLES.get(stem, title + " · pepe")
    crumbs = [("pepe", HOME), ("Docs", SITE)] + ([] if stem == "index" else [(title, canonical)])
    data = [
        {
            "@context": "https://schema.org",
            "@type": "TechArticle",
            "headline": title if stem != "index" else "pepe documentation",
            "description": description,
            "url": canonical,
            "image": OG_IMAGE,
            "inLanguage": "en",
            "author": AUTHOR,
            "publisher": AUTHOR,
            "isPartOf": {"@type": "WebSite", "name": "pepe", "url": HOME},
            "about": {"@type": "SoftwareApplication", "name": "pepe", "url": HOME},
        },
        {
            "@context": "https://schema.org",
            "@type": "BreadcrumbList",
            "itemListElement": [
                {"@type": "ListItem", "position": n + 1, "name": name, "item": url}
                for n, (name, url) in enumerate(crumbs)
            ],
        },
    ]
    ld = json.dumps(data, ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/")
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(full_title)}</title>
<meta name="description" content="{html.escape(description, quote=True)}">
<link rel="canonical" href="{canonical}">
<link rel="icon" href="../assets/logo.svg" type="image/svg+xml">
<meta name="color-scheme" content="light dark">
<meta name="theme-color" content="#ffffff" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#121010" media="(prefers-color-scheme: dark)">
<meta property="og:type" content="article">
<meta property="og:site_name" content="pepe">
<meta property="og:url" content="{canonical}">
<meta property="og:title" content="{html.escape(full_title, quote=True)}">
<meta property="og:description" content="{html.escape(description, quote=True)}">
<meta property="og:image" content="{OG_IMAGE}">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta property="og:image:alt" content="pepe: an HTTP load generator with a live terminal dashboard">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{html.escape(full_title, quote=True)}">
<meta name="twitter:description" content="{html.escape(description, quote=True)}">
<meta name="twitter:image" content="{OG_IMAGE}">
<script type="application/ld+json">{ld}</script>
<link rel="stylesheet" href="docs.css">
<script>try{{var t=localStorage.getItem('pepe-theme');if(t)document.documentElement.dataset.theme=t;}}catch(e){{}}</script>
</head>
<body>
<header class="top">
  <button type="button" class="menu" aria-label="Pages" aria-expanded="false">☰</button>
  <a class="brand" href="index.html"><img src="../assets/logo.svg" alt="" width="22" height="24">pepe<span>docs</span></a>
  <div class="search"><input type="search" placeholder="Search the docs…" aria-label="Search the docs" autocomplete="off"><div class="results" hidden></div></div>
  <nav class="links"><a href="../">Install</a><a href="{REPO}">GitHub</a><button type="button" class="theme" aria-label="Switch between light and dark">◐</button></nav>
</header>
<div class="shell">
  <aside class="side" id="side">{sidebar(stem)}</aside>
  <main class="main">
    <article class="prose">
{body}
    </article>
    {turn}
    <p class="edit"><a href="{REPO}/edit/master/docs/{stem}.md">Edit this page on GitHub</a></p>
  </main>
  <aside class="toc">{('<div class="label">On this page</div>' + toc) if toc else ''}</aside>
</div>
<script src="docs.js" defer></script>
</body>
</html>
"""


def build():
    pages, index = {}, []
    for stem, title in PAGES:
        path = os.path.join(SOURCE, stem + ".md")
        with open(path, encoding="utf-8") as f:
            text = f.read()
        body, headings, sections = convert(text)
        pages[stem + ".html"] = page(stem, title, body, headings, description(text))
        for s in sections:
            index.append({"p": stem + ".html", "n": title, "t": s["t"], "a": s["a"], "x": s["x"]})
    pages["search.json"] = json.dumps(index, ensure_ascii=False, separators=(",", ":")) + "\n"
    pages["../sitemap.xml"] = sitemap()
    return pages


def sitemap():
    """Every page the site serves, for search engines; site/robots.txt
    points here. No dates: a date written at build time would make the
    committed file stale on every merge."""
    urls = [HOME, SITE] + [SITE + stem + ".html" for stem, _ in PAGES if stem != "index"]
    out = ['<?xml version="1.0" encoding="UTF-8"?>',
           '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">']
    out += ["  <url><loc>%s</loc></url>" % html.escape(u) for u in urls]
    out.append("</urlset>")
    return "\n".join(out) + "\n"


def main():
    check = "--check" in sys.argv
    pages = build()
    stale = []
    os.makedirs(OUT, exist_ok=True)
    for name, content in pages.items():
        path = os.path.normpath(os.path.join(OUT, name))
        current = None
        if os.path.exists(path):
            with open(path, encoding="utf-8") as f:
                current = f.read()
        if current == content:
            continue
        if check:
            stale.append(name)
        else:
            with open(path, "w", encoding="utf-8") as f:
                f.write(content)
            print("wrote", os.path.relpath(path, ROOT))
    if check and stale:
        print("stale: " + ", ".join(stale) + "; run python3 site/build-docs.py")
        sys.exit(1)
    if check:
        print("site/docs is current")


if __name__ == "__main__":
    main()
