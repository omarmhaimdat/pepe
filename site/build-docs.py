#!/usr/bin/env python3
"""Build the documentation site: docs/*.md -> site/docs/*.html.

Plain Python, no packages: the Markdown pepe's docs use is small (headings,
paragraphs, fenced code, inline code, bold, links, images, tables, lists,
quotes), and a converter for that is shorter than a dependency. Every page
gets the install page's look (site/docs/docs.css), the sidebar from PAGES
below, and an "on this page" list from its headings.

    python3 site/build-docs.py           # write site/docs/
    python3 site/build-docs.py --check   # exit 1 when site/docs/ is stale
"""

import html
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCE = os.path.join(ROOT, "docs")
OUT = os.path.join(ROOT, "site", "docs")
SITE = "https://pepe.mhaimdat.com/docs/"

# The sidebar, in order: (file stem, title). A heading in the markdown is
# the page's title; this is what the sidebar calls it.
PAGES = [
    ("index", "Overview"),
    ("install", "Install"),
    ("load-test", "Load testing"),
    ("dashboard", "The dashboard"),
    ("ramp", "Ramp"),
    ("api", "OpenAPI"),
    ("flow", "Flows"),
    ("replay", "Replay"),
    ("ping", "Ping"),
    ("logs", "nginx logs"),
    ("compare", "Compare"),
    ("output", "Output and exit codes"),
    ("ci", "CI and Docker"),
    ("agents", "Agents and scripts"),
    ("benchmarks", "Benchmarks"),
    ("reference", "Command reference"),
]

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
    # A path into the repository (ROADMAP.md, bench/README.md, LICENSE)
    return "https://github.com/omarmhaimdat/pepe/blob/master/" + url.lstrip("./")


def inline(text):
    """Inline markup to HTML. Code spans are taken out first, so what is
    inside them is never read as markup."""
    spans = []

    def keep(match):
        spans.append("<code>%s</code>" % html.escape(match.group(1)))
        return "\0%d\0" % (len(spans) - 1)

    text = INLINE_CODE.sub(keep, text)
    text = html.escape(text, quote=False)
    text = IMAGE.sub(lambda m: '<img src="%s" alt="%s">' % (asset(m.group(2)), m.group(1)), text)
    text = LINK.sub(lambda m: '<a href="%s">%s</a>' % (href(m.group(2)), m.group(1)), text)
    text = BOLD.sub(r"<b>\1</b>", text)
    text = EM.sub(r"<em>\1</em>", text)
    return re.sub(r"\0(\d+)\0", lambda m: spans[int(m.group(1))], text)


def asset(url):
    """Images are the repository's assets/, which the site mirrors."""
    if url.startswith(("http://", "https://")):
        return url
    return "../" + url.lstrip("./")


def table(rows):
    cells = [[c.strip() for c in r.strip().strip("|").split("|")] for r in rows]
    head, body = cells[0], cells[2:]
    out = ["<table><thead><tr>"]
    out += ["<th>%s</th>" % inline(c) for c in head]
    out.append("</tr></thead><tbody>")
    for row in body:
        out.append("<tr>" + "".join("<td>%s</td>" % inline(c) for c in row) + "</tr>")
    out.append("</tbody></table>")
    return "".join(out)


def convert(text):
    """Markdown to HTML, and the headings for the page's own list."""
    lines = text.split("\n")
    out, headings = [], []
    i = 0
    paragraph = []

    def flush():
        if paragraph:
            out.append("<p>%s</p>" % inline(" ".join(paragraph)))
            paragraph.clear()

    while i < len(lines):
        line = lines[i]
        stripped = line.strip()
        if stripped.startswith("```"):
            flush()
            lang = stripped[3:].strip()
            i += 1
            code = []
            while i < len(lines) and not lines[i].strip().startswith("```"):
                code.append(lines[i])
                i += 1
            i += 1
            cls = ' class="lang-%s"' % html.escape(lang) if lang else ""
            out.append("<pre%s><code>%s</code></pre>" % (cls, html.escape("\n".join(code))))
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
                headings.append((level, title, anchor))
                out.append('<h%d id="%s">%s <a class="anchor" href="#%s" aria-label="Link to this section">#</a></h%d>'
                           % (level, anchor, inline(title), anchor, level))
            i += 1
            continue
        if stripped.startswith("|"):
            flush()
            rows = []
            while i < len(lines) and lines[i].strip().startswith("|"):
                rows.append(lines[i])
                i += 1
            out.append(table(rows))
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
            continue
        if stripped.startswith(">"):
            flush()
            quote = []
            while i < len(lines) and lines[i].strip().startswith(">"):
                quote.append(lines[i].strip()[1:].strip())
                i += 1
            inner, _ = convert("\n".join(quote))
            out.append("<blockquote>%s</blockquote>" % inner)
            continue
        if not stripped:
            flush()
            i += 1
            continue
        paragraph.append(stripped)
        i += 1
    flush()
    return "\n".join(out), headings


def first_paragraph(text):
    for block in text.split("\n\n"):
        block = block.strip()
        if block and not block.startswith(("#", "```", "|", "-", "!", ">")):
            return re.sub(r"[`*\[\]]", "", re.sub(r"\]\([^)]*\)", "]", block)).replace("\n", " ")[:300]
    return ""


def page(stem, title, body, headings, description):
    side = "".join(
        '<a href="%s.html"%s>%s</a>' % (s, ' class="on"' if s == stem else "", t) for s, t in PAGES
    )
    toc = "".join(
        '<a href="#%s" class="h%d">%s</a>' % (anchor, level, html.escape(re.sub(r"`", "", t)))
        for level, t, anchor in headings
        if level <= 3
    )
    canonical = SITE if stem == "index" else SITE + stem + ".html"
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)} · pepe docs</title>
<meta name="description" content="{html.escape(description, quote=True)}">
<link rel="canonical" href="{canonical}">
<link rel="icon" href="../assets/logo.svg" type="image/svg+xml">
<meta name="theme-color" content="#140f0d">
<meta property="og:type" content="article">
<meta property="og:title" content="{html.escape(title, quote=True)} · pepe docs">
<meta property="og:description" content="{html.escape(description, quote=True)}">
<meta property="og:image" content="https://pepe.mhaimdat.com/img/og.png">
<link rel="stylesheet" href="docs.css">
</head>
<body>
<div class="term">
  <header class="title">
    <a class="name" href="../">pepe</a><a href="index.html">docs</a>
    <nav aria-label="Site"><a href="../">install</a><a href="https://github.com/omarmhaimdat/pepe">github</a><a href="https://github.com/omarmhaimdat/pepe/releases">releases</a></nav>
  </header>
  <div class="cols">
    <aside class="side" aria-label="Pages">{side}</aside>
    <main>
{body}
    </main>
    <aside class="toc" aria-label="On this page">{('<div class="lab">on this page</div>' + toc) if toc else ''}</aside>
  </div>
  <footer class="chips"><span><kbd>?</kbd>every screen lists its keys</span><span><kbd>q</kbd>leaves the report in your shell</span><span class="by">MIT · <a href="https://github.com/omarmhaimdat/pepe">omarmhaimdat/pepe</a></span></footer>
</div>
</body>
</html>
"""


def build():
    pages = {}
    for stem, title in PAGES:
        path = os.path.join(SOURCE, stem + ".md")
        with open(path, encoding="utf-8") as f:
            text = f.read()
        body, headings = convert(text)
        pages[stem + ".html"] = page(stem, title, body, headings, first_paragraph(text))
    return pages


def main():
    check = "--check" in sys.argv
    pages = build()
    stale = []
    os.makedirs(OUT, exist_ok=True)
    for name, content in pages.items():
        path = os.path.join(OUT, name)
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
