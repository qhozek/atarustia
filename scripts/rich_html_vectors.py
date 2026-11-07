#!/usr/bin/env python3
"""Generate pure renderer vectors from pinned upstream, without loading the userbot.

Unlike the static inventory, this explicitly executes an allow-listed extracted
HTML parser in a small synthetic namespace. No Telegram imports/clients/login.
"""
import argparse
import ast
import html
from html.parser import HTMLParser
import pathlib
import re
from types import SimpleNamespace
import typing

from upstream_contract import Source, ROOT, canonical, load, MANIFEST


CASES = [
    ("blocks", 'head\n<details><summary>s</summary>a\nb</details>\n<table><tr><td>x</td></tr></table>\ntail'),
    ("pre", 'a\n<pre><code>x\n  y</code></pre>\nb\nc'),
    ("sample", '<b>T</b> <tg-button type="disabled">3</tg-button> <tg-button type="callback_data" data="open">mod</tg-button>\n<details><summary>sum</summary>a\n<blockquote>inner</blockquote><tg-button type="copy_text" text="v">📋</tg-button></details>\n<table><tr><th>k</th><td><code>v</code></td></tr></table>\n<tg-button-row><tg-button type="callback_data" data="plus">+1</tg-button><tg-button type="disabled">5</tg-button></tg-button-row><img src="https://a.b/c.png"/>'),
    ("open-details", '<details open><summary><b>Title</b></summary>First<br>Second</details>'),
    ("nested-quotes", '<blockquote expandable>a<blockquote>b</blockquote>c</blockquote>'),
    ("table", '<table bordered compact><tr><th>Key</th><td>Value</td><td>Other</td></tr><tr><td>A</td><td>B</td></tr></table>'),
    ("lists", '<h2>Heading</h2><ul><li>A</li><li><i>B</i></li></ul><hr/>tail'),
    ("entities", '<b>A &amp; B &lt;x&gt;</b> &#128578; <a href="https://a.b/?x=1&amp;y=2">link</a>'),
    ("copy-url", 'before<tg-button type="copy_text" text="a&amp;b">Copy</tg-button><tg-button type="url" url="https://a.b/?x=1&amp;y=2">Open</tg-button>after'),
    ("row-chunks", '<tg-button-row>' + ''.join(f'<tg-button type="disabled">{i}</tg-button>' for i in range(10)) + '</tg-button-row>'),
    ("stray-end", 'a</tg-button>b'),
    ("nested-button", '<tg-button type="url" url="https://a.b"><tg-button>i</tg-button>o</tg-button>'),
    ("multiline", '\n\nhello\nworld\n\n\nnext\n'),
    ("pre-entities", '<pre><code class="language-rust">a &lt; b\n  &quot;x&quot;</code></pre>'),
    ("emoji", '<tg-emoji emoji-id="123">★</tg-emoji><tg-spoiler>hide</tg-spoiler>'),
    ("quoted-attribute", '<a href="https://example.com/?q=&#39;x&#39;&amp;s=&quot;y&quot;">"quoted"</a>'),
    ("uppercase", 'head\n<DETAILS OPEN><SUMMARY>Title</SUMMARY>A<BR/>B</DETAILS>\n<PRE>x\n y</PRE>\ntail'),
    ("nested-details", '<details><summary>Outer</summary>A<details open><summary>Inner</summary>B</details>C</details>'),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=pathlib.Path, required=True)
    args = parser.parse_args()
    revision = load(MANIFEST)['upstream']['revision']
    source = Source(args.source, revision)
    path = 'ataraxia/inline/screens.py'
    tree = source.tree(path)
    names = {'escape', 'attr', 'strip_tags', 'to_rich', 'to_plain', '_Plain'}
    constants = {'_BLOCK', '_BLOCK_RE', '_TAG_RE', '_PRE_RE', 'ROW_LIMIT', 'KEYBOARD_LIMIT'}
    nodes = []
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.ClassDef)) and node.name in names:
            nodes.append(node)
        elif isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id in constants for t in node.targets):
            nodes.append(node)
    namespace = {'html': html, 'HTMLParser': HTMLParser, 're': re, 'typing': typing,
                 'utils': SimpleNamespace(chunks=lambda items, size: [items[i:i+size] for i in range(0, len(items), size)])}
    exec(compile(ast.Module(body=nodes, type_ignores=[]), path, 'exec'), namespace)
    vectors = []
    for name, html_source in CASES:
        plain, rows = namespace['to_plain'](html_source)
        vectors.append({'name': name, 'source': html_source, 'rich': namespace['to_rich'](html_source),
                        'plain': plain, 'rows': rows})
    target = ROOT / 'tests/fixtures/rich_html.json'
    target.write_bytes(canonical({'revision': revision, 'source': path, 'vectors': vectors}))
    print(f'Generated {len(vectors)} pure upstream renderer vectors')


if __name__ == '__main__':
    main()
