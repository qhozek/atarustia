#!/usr/bin/env python3
"""Extract pinned Screen icon constants with literal_eval, never import upstream."""
import argparse
import ast
from pathlib import Path
from upstream_contract import Source, ROOT, MANIFEST, load, canonical


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    args = parser.parse_args()
    revision = load(MANIFEST)['upstream']['revision']
    tree = Source(args.source, revision).tree('ataraxia/inline/screens.py')
    icons = next(ast.literal_eval(node.value) for node in tree.body
                 if isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name)
                 and node.target.id == 'ICONS')
    vectors = [{'name': name, 'id': str(value[0]), 'alt': value[1], 'fallback': value[2]}
               for name, value in sorted(icons.items())]
    (ROOT / 'tests/fixtures/emoji.json').write_bytes(canonical({'revision': revision, 'icons': vectors}))
    print(f'Extracted {len(vectors)} original Screen icons')


if __name__ == '__main__':
    main()
