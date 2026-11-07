#!/usr/bin/env python3
"""Static upstream contract inventory. Never import or execute upstream Python.

Only Python's standard library and Git are needed. Read committed blobs rather
than a checkout: Windows line-ending conversion must not change the contract.
"""
from __future__ import annotations

import argparse
import ast
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tempfile
import os

ROOT = pathlib.Path(__file__).resolve().parents[1]
REPOSITORY = "https://git.break.wtf/xenside/ataraxia.git"
MANIFEST = ROOT / "contracts/upstream-manifest.json"
POLICY = ROOT / "contracts/port-policy.json"
ARGUMENTS = ROOT / "contracts/command-behavior.json"
SCHEMA_VERSION = 1
EXTRACTOR_VERSION = 1
PYTHON_MINOR = (3, 14)
MAX_BLOB = 4 * 1024 * 1024


class ContractError(Exception):
    pass


def git(source, *args, timeout=90):
    result = subprocess.run(
        ["git", "-C", str(source), *args], stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout,
        env={**os.environ, "GIT_TERMINAL_PROMPT": "0"},
    )
    if result.returncode:
        # Do not echo arbitrary repository configuration or remote output.
        raise ContractError(f"Git {args[0]} failed (exit {result.returncode})")
    return result.stdout


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode()


def load(path):
    return json.loads(pathlib.Path(path).read_text(encoding="utf-8"))


class Source:
    def __init__(self, directory, revision):
        if not re.fullmatch(r"[0-9a-f]{40}", revision):
            raise ContractError("revision must be a full lowercase Git SHA")
        self.directory = directory
        self.revision = revision
        identity = git(directory, "rev-parse", "--verify", revision + "^{commit}").decode().strip()
        if identity != revision:
            raise ContractError("commit identity mismatch")
        tree = git(directory, "ls-tree", "-rz", "--full-tree", revision)
        self.files = {}
        for record in tree.split(b"\0"):
            if not record:
                continue
            metadata, path = record.split(b"\t", 1)
            mode, kind, oid = metadata.decode().split()
            if kind in ("blob", "commit"):
                self.files[path.decode("utf-8")] = (mode, oid)
        self.cache = {}

    def read(self, path):
        if path not in self.cache:
            mode, oid = self.files[path]
            if mode not in ("100644", "100755"):
                raise ContractError(f"not a regular upstream file: {path}")
            size = int(git(self.directory, "cat-file", "-s", oid))
            if size > MAX_BLOB:
                raise ContractError(f"upstream file exceeds inventory limit: {path}")
            self.cache[path] = git(self.directory, "cat-file", "blob", oid)
        return self.cache[path]

    def text(self, path):
        return self.read(path).decode("utf-8")

    def tree(self, path):
        return ast.parse(self.text(path), filename=path)

    def file_contracts(self):
        # Watch the whole tracked tree, including tests/docs/workflows. Changes
        # outside the extracted API still require human review.
        result = {}
        for path, (mode, oid) in sorted(self.files.items()):
            result[path] = {"mode": mode, "git_blob": oid}
            if mode in ("100644", "100755"):
                result[path]["sha256"] = digest(self.read(path))
        return result


def expression(node):
    return ast.unparse(node) if node is not None else None


def value(node, symbols=None):
    """Return an explicit expression marker instead of evaluating dynamic code."""
    if node is None:
        return None
    try:
        return ast.literal_eval(node)
    except (ValueError, TypeError):
        name = expression(node)
        if symbols and name in symbols:
            return symbols[name]
        return {"expression": name, "resolved": False}


def fingerprint(node):
    return digest(ast.dump(node, include_attributes=False).encode())


def signature(node):
    args = node.args
    positional = args.posonlyargs + args.args
    defaults = [None] * (len(positional) - len(args.defaults)) + list(args.defaults)
    parameters = []
    for index, (arg, default) in enumerate(zip(positional, defaults)):
        parameters.append({
            "name": arg.arg,
            "kind": "positional_only" if index < len(args.posonlyargs) else "positional_or_keyword",
            "annotation": expression(arg.annotation),
            "required": default is None,
            "default": expression(default),
        })
    if args.vararg:
        parameters.append({"name": args.vararg.arg, "kind": "varargs", "annotation": expression(args.vararg.annotation)})
    for arg, default in zip(args.kwonlyargs, args.kw_defaults):
        parameters.append({"name": arg.arg, "kind": "keyword_only", "annotation": expression(arg.annotation),
                           "required": default is None, "default": expression(default)})
    if args.kwarg:
        parameters.append({"name": args.kwarg.arg, "kind": "kwargs", "annotation": expression(args.kwarg.annotation)})
    return {"async": isinstance(node, ast.AsyncFunctionDef), "parameters": parameters,
            "returns": expression(node.returns)}


def method(node, path):
    return {"source": path, "line": node.lineno, "signature": signature(node),
            "decorators": [expression(d) for d in node.decorator_list],
            "documentation": ast.get_docstring(node), "ast_sha256": fingerprint(node)}


def scalar_langpack(text):
    """Strict subset used by the pinned packs; fail if upstream changes grammar.

    Never silently approximate general YAML: multi-line/unquoted scalars need an
    extractor update and review rather than an incorrect manifest.
    """
    result = {}
    section = None
    for number, raw in enumerate(text.splitlines(), 1):
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        top = re.fullmatch(r"(\$?[\w]+):\s*", raw)
        if top:
            section = top[1]
            if section in result:
                raise ContractError(f"duplicate langpack section at line {number}")
            result[section] = {}
            continue
        item = re.fullmatch(r'''  ("[\w-]+"|'[\w-]+'|[\w-]+):\s*(.+?)\s*''', raw)
        if not item or section is None:
            raise ContractError(f"unsupported langpack grammar at line {number}")
        key, literal = item.groups()
        key = key.strip("\"'")
        if key in result[section]:
            raise ContractError(f"duplicate langpack key at line {number}")
        if literal.startswith('"'):
            try:
                parsed = json.loads(literal)
            except json.JSONDecodeError as error:
                raise ContractError(f"unsupported YAML escape at line {number}") from error
        elif len(literal) >= 2 and literal.startswith("'") and literal.endswith("'"):
            parsed = literal[1:-1].replace("''", "'")
        else:
            raise ContractError(f"unsupported langpack scalar at line {number}")
        if not isinstance(parsed, str):
            raise ContractError(f"non-string langpack scalar at line {number}")
        result[section][key] = parsed
    return result


def security(source):
    tree = source.tree("ataraxia/security.py")
    bits = {}
    for node in tree.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            name = node.targets[0].id
            if isinstance(node.value, ast.BinOp) and isinstance(node.value.op, ast.LShift):
                left, right = value(node.value.left), value(node.value.right)
                if isinstance(left, int) and isinstance(right, int):
                    bits[name] = left << right
    default = next(expression(n.value) for n in tree.body if isinstance(n, ast.Assign)
                   and any(isinstance(t, ast.Name) and t.id == "DEFAULT_PERMISSIONS" for t in n.targets))
    return {"bits": bits, "default_expression": default, "default_mask": bits[default],
            "source": "ataraxia/security.py", "ast_sha256": fingerprint(tree)}


def public_apis(source, path, classes=None, include_private=False):
    result = {}
    for cls in source.tree(path).body:
        if not isinstance(cls, ast.ClassDef) or classes is not None and cls.name not in classes:
            continue
        methods = {n.name: method(n, path) for n in cls.body
                   if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)) and (include_private or not n.name.startswith("_"))}
        result[cls.name] = {"source": path, "line": cls.lineno, "bases": [expression(b) for b in cls.bases],
                            "methods": methods, "ast_sha256": fingerprint(cls)}
    return result


def events(source):
    path = "ataraxia/inline/events.py"
    tree = source.tree(path)
    raw = next(n.value for n in tree.body if isinstance(n, ast.Assign)
               and any(isinstance(t, ast.Name) and t.id == "RAW_EVENTS" for t in n.targets))
    if not isinstance(raw, ast.DictComp):
        raise ContractError("RAW_EVENTS extraction must be reviewed")
    pairs = value(raw.generators[0].iter)
    if not isinstance(pairs, tuple) or not all(isinstance(p, tuple) and len(p) == 2 for p in pairs):
        raise ContractError("RAW_EVENTS is no longer a literal pair table")
    handled = next(n.value for n in tree.body if isinstance(n, ast.Assign)
                   and any(isinstance(t, ast.Name) and t.id == "HANDLED_UPDATES" for t in n.targets))
    return {"source": path, "raw_updates": dict(pairs),
            "handled_updates": [expression(n) for n in handled.elts],
            "handlers": public_apis(source, path, include_private=True), "ast_sha256": fingerprint(tree)}


def cli(source):
    path = "ataraxia/main.py"
    tree = source.tree(path)
    parser = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "parse_arguments")
    options = []
    for call in ast.walk(parser):
        if isinstance(call, ast.Call) and isinstance(call.func, ast.Attribute) and call.func.attr == "add_argument":
            options.append({"flags": [value(n) for n in call.args],
                            "options": {k.arg: value(k.value) for k in call.keywords}, "line": call.lineno})
    return {"source": path, "options": options, "ast_sha256": fingerprint(parser),
            "unknown_arguments": "parse_known_args: ignore and warn", "login": "terminal; own API pair; QR default or phone/code/2FA"}


def screen_contract(source):
    path = "ataraxia/inline/screens.py"
    constants = {}
    for node in source.tree(path).body:
        if isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name) and target.id in ("COPY_LIMIT", "ROW_LIMIT", "KEYBOARD_LIMIT", "STYLES"):
                    item = value(node.value)
                    constants[target.id] = sorted(item) if isinstance(item, set) else item
        if isinstance(node, ast.AnnAssign) and expression(node.target) == "ICONS":
            constants["ICONS"] = value(node.value)
    return {"source": path, "constants": constants, "builder": public_apis(source, path, {"Screen"})}


def generate(source, arguments, policy):
    if arguments.get("schema_version") != 1 or policy.get("schema_version") != 1:
        raise ContractError("unsupported behavior/policy schema")
    packs = {lang: scalar_langpack(source.text(f"ataraxia/langpacks/{lang}.yml")) for lang in ("en", "ru")}
    sec = security(source)
    modules, commands, configs = {}, {}, {}
    symbols = {}
    for node in source.tree("ataraxia/utils/ataraxia.py").body:
        if isinstance(node, ast.Assign) and isinstance(node.value, ast.Constant):
            for target in node.targets:
                if isinstance(target, ast.Name):
                    symbols[f"utils.{target.id}"] = node.value.value
    for path in sorted(source.files):
        if not re.fullmatch(r"ataraxia/modules/[^/]+\.py", path):
            continue
        section = pathlib.PurePosixPath(path).stem
        for cls in source.tree(path).body:
            if not isinstance(cls, ast.ClassDef) or not any(expression(b) == "loader.Module" for b in cls.bases):
                continue
            strings = next((value(n.value) for n in cls.body if isinstance(n, ast.Assign)
                            and any(isinstance(t, ast.Name) and t.id == "strings" for t in n.targets)), None)
            if not isinstance(strings, dict) or not isinstance(strings.get("name"), str):
                raise ContractError(f"module name requires extractor review: {path}:{cls.name}")
            name = strings["name"]
            if name in modules:
                raise ContractError(f"duplicate module: {name}")
            modules[name] = {"class": cls.name, "source": path, "line": cls.lineno,
                             "description": ast.get_docstring(cls), "commands": [], "config": [],
                             "ast_sha256": fingerprint(cls)}
            for node in cls.body:
                if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    continue
                decorator = next((d for d in node.decorator_list if isinstance(d, ast.Call)
                                  and expression(d.func) == "loader.command"), None)
                if decorator is None and not node.name.endswith("cmd"):
                    continue
                command = node.name.removesuffix("cmd")
                if command in commands:
                    raise ContractError(f"duplicate command: {command}")
                aliases = []
                if decorator:
                    for keyword in decorator.keywords:
                        if keyword.arg in ("alias", "aliases"):
                            item = value(keyword.value)
                            aliases.extend([item] if isinstance(item, str) else item)
                decorators = [expression(d) for d in node.decorator_list]
                # Fail closed instead of pretending a new security decorator is OWNER.
                if any(d.startswith(("loader.owner", "loader.unrestricted", "loader.group_", "loader.pm", "loader.inline_everyone", "loader.sudo", "loader.support", "security.")) for d in decorators) or (
                    decorator and any(k.arg in ("security", "permission", "permissions") for k in decorator.keywords)
                ):
                    raise ContractError(f"new command security decorator requires review: {command}")
                docs = {lang: packs[lang].get(section, {}).get(f"_cmd_doc_{command}", ast.get_docstring(node)) for lang in packs}
                commands[command] = {**method(node, path), "module": name, "method": node.name,
                                     "aliases": aliases, "security": {"mask": sec["default_mask"], "origin": "DEFAULT_PERMISSIONS; no security decorators"},
                                     "help": docs, "argument_contract": arguments["commands"].get(command)}
                modules[name]["commands"].append(command)
            # Walk only __init__ config calls, preserving declaration order.
            init = next((n for n in cls.body if isinstance(n, ast.FunctionDef) and n.name == "__init__"), None)
            calls = sorted((n for n in ast.walk(init) if isinstance(n, ast.Call) and expression(n.func) == "loader.ConfigValue"),
                           key=lambda n: (n.lineno, n.col_offset)) if init else []
            for call in calls:
                key = value(call.args[0])
                default = call.args[1] if len(call.args) > 1 else next((k.value for k in call.keywords if k.arg == "default"), None)
                doc = call.args[2] if len(call.args) > 2 else next((k.value for k in call.keywords if k.arg == "doc"), None)
                validator = next((k.value for k in call.keywords if k.arg == "validator"), None)
                identity = f"{name}.{key}"
                configs[identity] = {"module": name, "key": key, "source": path, "line": call.lineno,
                                     "order": len(modules[name]["config"]), "default": value(default, symbols),
                                     "default_expression": expression(default), "documentation_expression": expression(doc),
                                     "validator": expression(validator),
                                     "on_change": next((expression(k.value) for k in call.keywords if k.arg == "on_change"), None),
                                     "ast_sha256": fingerprint(call)}
                modules[name]["config"].append(identity)
    missing = sorted(set(commands) - set(arguments["commands"]))
    obsolete = sorted(set(arguments["commands"]) - set(commands))
    # For drift inventory retain missing/new command contracts explicitly; generation
    # of an accepted pin is gated separately by validate_manifest().
    return {"schema_version": SCHEMA_VERSION, "extractor_version": EXTRACTOR_VERSION,
            "upstream": {"repository": REPOSITORY, "branch": "main", "revision": source.revision},
            "extraction": {"mode": "static AST and strict quoted YAML scalars; no imports/eval",
                           "argument_semantics": "human-reviewed scenarios, not inferred/proven from signatures",
                           "arguments_reviewed_revision": arguments.get("reviewed_revision"),
                           "missing_argument_contracts": missing, "obsolete_argument_contracts": obsolete},
            "policy": policy, "files": source.file_contracts(), "modules": modules,
            "commands": commands, "configs": configs, "security": sec,
            "strings": packs,
            "assets": {path: {"sha256": digest(source.read(path)), "bytes": len(source.read(path))}
                       for path in sorted(source.files) if path.startswith("assets/")},
            "cli": cli(source), "screens": screen_contract(source),
            "bot_facade": public_apis(source, "ataraxia/inline/bot.py", {"InlineBot"}),
            "inline_api": {path: public_apis(source, path) for path in (
                "ataraxia/inline/form.py", "ataraxia/inline/list.py", "ataraxia/inline/guest.py",
                "ataraxia/inline/types.py", "ataraxia/inline/screens.py")},
            "events": events(source),
            "module_api": public_apis(source, "ataraxia/types.py", {"Module", "Library", "ModuleConfig", "ConfigValue"}),
            "validators": public_apis(source, "ataraxia/validators.py")}


def validate_manifest(manifest):
    if manifest.get("schema_version") != SCHEMA_VERSION or manifest.get("extractor_version") != EXTRACTOR_VERSION:
        raise ContractError("unsupported manifest schema/extractor version")
    if manifest["extraction"]["arguments_reviewed_revision"] != manifest["upstream"]["revision"]:
        raise ContractError("command behavior review is for another revision; review before accepting a new pin")
    if manifest["extraction"]["missing_argument_contracts"] or manifest["extraction"]["obsolete_argument_contracts"]:
        raise ContractError("command behavior inventory does not cover this revision; review added/removed commands")
    aliases = {}
    for name, cmd in manifest["commands"].items():
        contract = cmd["argument_contract"]
        if not contract or not all(contract.get(k) for k in ("syntax", "scenarios", "effects")):
            raise ContractError(f"incomplete reviewed argument contract: {name}")
        for alias in cmd["aliases"]:
            if alias in aliases or alias in manifest["commands"]:
                raise ContractError(f"alias collision: {alias}")
            aliases[alias] = name


def changed_keys(old, new):
    return {"added": sorted(new.keys() - old.keys()), "removed": sorted(old.keys() - new.keys()),
            "changed": sorted(k for k in old.keys() & new.keys() if old[k] != new[k])}


def difference(old, new):
    # AST literals contain tuples; a saved JSON manifest necessarily has arrays.
    # Normalize before comparison to avoid reporting false drift after reload.
    old, new = json.loads(canonical(old)), json.loads(canonical(new))
    sections = {}
    for section in ("files", "modules", "commands", "configs", "assets", "bot_facade", "inline_api", "module_api", "validators"):
        change = changed_keys(old[section], new[section])
        if any(change.values()):
            sections[section] = change
    for language in sorted(old["strings"].keys() | new["strings"].keys()):
        flat = lambda data: {f"{section}.{key}": text for section, values in data.items() for key, text in values.items()}
        change = changed_keys(flat(old["strings"].get(language, {})), flat(new["strings"].get(language, {})))
        if any(change.values()):
            sections[f"strings.{language}"] = change
    for section in ("events", "security", "policy", "extraction", "cli", "screens"):
        if old[section] != new[section]:
            sections[section] = {"changed": [section]}
    return {"baseline": old["upstream"]["revision"], "candidate": new["upstream"]["revision"],
            "revision_changed": old["upstream"]["revision"] != new["upstream"]["revision"],
            "sections": sections, "implementation_status": "not evaluated; differences require review"}


def markdown(report):
    lines = ["# Ataraxia upstream drift", "", f"Baseline: `{report['baseline']}`", f"Candidate: `{report['candidate']}`", "",
             "This report does not update the accepted pin or assert Rust behavioral parity.", ""]
    if report.get("extraction_error"):
        lines.extend(["## Extraction failed — manual review required", "", report["extraction_error"], ""])
    if not report["revision_changed"] and not report["sections"]:
        lines.append("No drift detected.")
    for section, changes in report["sections"].items():
        lines.extend([f"## {section}", ""])
        for kind, names in changes.items():
            if names:
                lines.append(f"- {kind} ({len(names)}): " + ", ".join(f"`{name}`" for name in names))
        lines.append("")
    return "\n".join(lines) + "\n"


def remote_head():
    result = git(ROOT, "ls-remote", REPOSITORY, "refs/heads/main").decode().strip().split()
    if len(result) != 2 or result[1] != "refs/heads/main" or not re.fullmatch(r"[0-9a-f]{40}", result[0]):
        raise ContractError("invalid upstream main identity")
    return result[0]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["generate", "verify", "drift"])
    parser.add_argument("--source", type=pathlib.Path, help="local upstream Git object store (never imported/executed)")
    parser.add_argument("--revision", help="explicit candidate commit; drift defaults to remote main")
    parser.add_argument("--manifest", type=pathlib.Path, default=MANIFEST)
    parser.add_argument("--output", type=pathlib.Path, help="explicit output for generate; JSON drift report for drift")
    parser.add_argument("--markdown", type=pathlib.Path, help="Markdown drift report")
    args = parser.parse_args(argv)
    if sys.version_info[:2] != PYTHON_MINOR:
        raise ContractError("accepted AST inventory uses Python 3.14; use the same minor version as CI")
    baseline = load(args.manifest) if args.mode != "generate" else None
    revision = args.revision or (remote_head() if args.mode == "drift" else baseline["upstream"]["revision"] if baseline else None)
    if not revision:
        parser.error("generate requires --revision")
    try:
        with tempfile.TemporaryDirectory(prefix="atarustia-upstream-") as temporary:
            directory = args.source
            if directory is None:
                directory = pathlib.Path(temporary)
                git(directory, "init", "--bare", "--quiet")
                git(directory, "fetch", "--quiet", "--depth=1", REPOSITORY, revision)
            current = generate(Source(directory, revision), load(ARGUMENTS), load(POLICY))
    except (ContractError, KeyError, ValueError, SyntaxError, OSError, subprocess.TimeoutExpired) as error:
        if args.mode != "drift":
            raise
        report = {"baseline": baseline["upstream"]["revision"], "candidate": revision,
                  "revision_changed": baseline["upstream"]["revision"] != revision,
                  "sections": {}, "extraction_error": str(error),
                  "implementation_status": "not evaluated; extraction must be updated before reviewing drift"}
        if args.output:
            args.output.write_bytes(canonical(report))
        if args.markdown:
            args.markdown.write_text(markdown(report), encoding="utf-8")
        print(markdown(report))
        return 2
    if args.mode == "generate":
        if args.output is None:
            parser.error("generate requires --output; accepted manifest is never overwritten implicitly")
        validate_manifest(current)
        args.output.write_bytes(canonical(current))
        print(f"Generated {len(current['commands'])} commands, {len(current['modules'])} modules, {len(current['configs'])} config values")
        return 0
    if baseline["schema_version"] != SCHEMA_VERSION or baseline["extractor_version"] != EXTRACTOR_VERSION:
        raise ContractError("manifest schema/extractor version requires explicit migration")
    report = difference(baseline, current)
    if args.mode == "verify":
        validate_manifest(current)
        if canonical(baseline) != canonical(current):
            print(markdown(report))
            raise ContractError("accepted manifest is stale or edited; regenerate/review explicitly")
        print(f"Pinned upstream manifest verified: {revision}")
        return 0
    if args.output:
        args.output.write_bytes(canonical(report))
    text = markdown(report)
    if args.markdown:
        args.markdown.write_text(text, encoding="utf-8")
    print(text)
    return 1 if report["revision_changed"] or report["sections"] else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ContractError, KeyError, ValueError, SyntaxError, OSError, subprocess.TimeoutExpired) as error:
        print(f"upstream contract check failed: {error}", file=sys.stderr)
        sys.exit(2)
