"""Offline tests for inventory, explicit failures and drift reporting."""
import json
import pathlib
import tempfile
import unittest
from unittest.mock import patch
import contextlib
import io

import upstream_contract as contract


class SyntheticSource:
    revision = "a" * 40
    data = {
        "ataraxia/security.py": "OWNER = 1 << 0\nEVERYONE = 1 << 13\nDEFAULT_PERMISSIONS = OWNER\n",
        "ataraxia/utils/ataraxia.py": 'BANNER_URL = "https://example.com/banner.png"\n',
        "ataraxia/modules/demo.py": '''
class Demo(loader.Module):
    """Demo module"""
    strings = {"name": "Demo"}
    def __init__(self):
        self.config = loader.ModuleConfig(loader.ConfigValue("enabled", False, validator=loader.validators.Boolean()))
    @loader.command(alias="d")
    async def democmd(self, message):
        """[args] - demo"""
        pass
''',
        "ataraxia/langpacks/en.yml": 'demo:\n  _cmd_doc_demo: "[args] - translated"\n  "on": "On"\n',
        "ataraxia/langpacks/ru.yml": 'demo:\n  _cmd_doc_demo: "[args] - translated"\n  "on": "On"\n',
        "ataraxia/inline/bot.py": 'class InlineBot:\n    async def send(self, chat, text="", *, silent=False, **kwargs): pass\n',
        "ataraxia/inline/events.py": '''
RAW_EVENTS = {getattr(tl, name): event for name, event in (("UpdateWidget", "widget"),) if hasattr(tl, name)}
HANDLED_UPDATES = (tl.UpdateQuery, *RAW_EVENTS)
class Events:
    async def _process_update(self, update): pass
''',
        "ataraxia/main.py": '''
def parse_arguments():
    parser.add_argument("--no-auth", action="store_true")
    return parser.parse_known_args()
''',
        "ataraxia/inline/screens.py": 'ICONS: dict = {"close": (1, "X", "x")}\nKEYBOARD_LIMIT = 100\nSTYLES = {"danger", "link"}\nclass Screen: pass\n',
        "ataraxia/types.py": 'class Module: pass\nclass Library: pass\nclass ModuleConfig: pass\nclass ConfigValue: pass\n',
        "ataraxia/validators.py": 'class Boolean: pass\n',
        "ataraxia/inline/form.py": 'class Form:\n    async def form(self, text, ttl=None): pass\n',
        "ataraxia/inline/list.py": 'class List: pass\n',
        "ataraxia/inline/guest.py": 'class Guest: pass\n',
        "ataraxia/inline/types.py": 'class InlineCall: pass\n',
        "assets/banner.txt": "synthetic\n",
    }

    def __init__(self):
        self.data = dict(self.data)
        self.files = {name: ("100644", "0" * 40) for name in self.data}

    def text(self, path):
        return self.data[path]

    def read(self, path):
        return self.text(path).encode()

    def tree(self, path):
        return contract.ast.parse(self.text(path))

    def file_contracts(self):
        return {name: {"mode": "100644", "git_blob": "0" * 40, "sha256": contract.digest(self.read(name))}
                for name in sorted(self.data)}


def generate(source=None):
    return contract.generate(source or SyntheticSource(), {
        "schema_version": 1, "reviewed_revision": "a" * 40,
        "commands": {"demo": {"syntax": "[args]", "scenarios": ["empty"], "effects": ["answer"]}},
    }, {"schema_version": 1, "unresolved_decisions": ["synthetic"]})


class InventoryTests(unittest.TestCase):
    def test_extracts_names_aliases_signatures_defaults_and_events_without_execution(self):
        source = SyntheticSource()
        source.data["ataraxia/modules/demo.py"] += '\nraise RuntimeError("must never execute")\n'
        m = generate(source)
        contract.validate_manifest(m)
        self.assertEqual(m["commands"]["demo"]["aliases"], ["d"])
        self.assertEqual(m["commands"]["demo"]["module"], "Demo")
        self.assertEqual(m["commands"]["demo"]["security"]["mask"], 1)
        self.assertIs(m["configs"]["Demo.enabled"]["default"], False)
        self.assertEqual(m["events"]["raw_updates"], {"UpdateWidget": "widget"})
        self.assertIn("_process_update", m["events"]["handlers"]["Events"]["methods"])
        params = m["bot_facade"]["InlineBot"]["methods"]["send"]["signature"]["parameters"]
        self.assertEqual(params[-1]["kind"], "kwargs")
        self.assertEqual(params[-2]["default"], "False")

    def test_dynamic_defaults_are_unresolved_not_evaled(self):
        node = contract.ast.parse('dangerous_call()', mode="eval").body
        self.assertEqual(contract.value(node), {"expression": "dangerous_call()", "resolved": False})

    def test_yaml_quoted_keys_and_strings_are_lossless(self):
        parsed = contract.scalar_langpack('demo:\n  "on": "On\\nline"\n  key: \'it\'\'s quoted\'\n')
        self.assertEqual(parsed, {"demo": {"on": "On\nline", "key": "it's quoted"}})

    def test_yaml_unknown_grammar_and_duplicate_keys_fail_closed(self):
        for text in ['demo:\n  key: |\n    multiline\n', 'demo:\n  key: true\n',
                     'demo:\n  key: "x"\n  key: "y"\n', 'demo:\ndemo:\n']:
            with self.subTest(text=text), self.assertRaises(contract.ContractError):
                contract.scalar_langpack(text)

    def test_security_decorator_does_not_silently_inherit_owner(self):
        source = SyntheticSource()
        source.data["ataraxia/modules/demo.py"] = source.data["ataraxia/modules/demo.py"].replace(
            '@loader.command(alias="d")', '@loader.unrestricted\n    @loader.command(alias="d")')
        with self.assertRaises(contract.ContractError):
            generate(source)

    def test_new_removed_commands_require_manual_argument_review(self):
        source = SyntheticSource()
        source.data["ataraxia/modules/demo.py"] = source.data["ataraxia/modules/demo.py"].replace("democmd", "newcmd")
        m = generate(source)
        self.assertEqual(m["extraction"]["missing_argument_contracts"], ["new"])
        self.assertEqual(m["extraction"]["obsolete_argument_contracts"], ["demo"])
        with self.assertRaises(contract.ContractError):
            contract.validate_manifest(m)

    def test_revision_requires_review_even_when_extracted_names_are_unchanged(self):
        m = generate()
        m["upstream"]["revision"] = "b" * 40
        with self.assertRaises(contract.ContractError):
            contract.validate_manifest(m)

    def test_drift_reports_bodies_strings_assets_and_unclassified_files(self):
        old, new = generate(), generate()
        new["commands"]["demo"]["ast_sha256"] = "changed-body"
        new["strings"]["en"]["demo"]["on"] = "New"
        new["assets"]["assets/banner.txt"]["sha256"] = "changed-image"
        new["files"]["tests/new.py"] = {"git_blob": "1" * 40}
        report = contract.difference(old, new)
        self.assertEqual(report["sections"]["commands"]["changed"], ["demo"])
        self.assertEqual(report["sections"]["strings.en"]["changed"], ["demo.on"])
        self.assertEqual(report["sections"]["files"]["added"], ["tests/new.py"])
        self.assertIn("not evaluated", report["implementation_status"])

    def test_identical_generation_is_deterministic(self):
        self.assertEqual(contract.canonical(generate()), contract.canonical(generate()))
        self.assertFalse(contract.difference(generate(), generate())["sections"])
        self.assertFalse(contract.difference(json.loads(contract.canonical(generate())), generate())["sections"])

    def test_cli_verify_rejects_edited_manifest_and_drift_returns_one_without_repinning(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            baseline = root / "baseline.json"
            baseline.write_bytes(contract.canonical(generate()))
            original = baseline.read_bytes()
            candidate = SyntheticSource()
            candidate.revision = "b" * 40
            with patch.object(contract, "Source", return_value=candidate), patch.object(contract, "ARGUMENTS", root / "arguments.json"), patch.object(contract, "POLICY", root / "policy.json"), contextlib.redirect_stdout(io.StringIO()):
                (root / "arguments.json").write_bytes(contract.canonical({"schema_version":1, "reviewed_revision":"a"*40,
                    "commands":{"demo":{"syntax":"[args]", "scenarios":["empty"], "effects":["answer"]}}}))
                (root / "policy.json").write_bytes(contract.canonical({"schema_version":1, "unresolved_decisions":["synthetic"]}))
                code = contract.main(["drift", "--source", str(root), "--manifest", str(baseline), "--revision", "b"*40])
                self.assertEqual(code, 1)
                self.assertEqual(baseline.read_bytes(), original)
                edited = generate()
                edited["commands"]["demo"]["aliases"] = ["edited"]
                baseline.write_bytes(contract.canonical(edited))
                with patch.object(contract, "Source", return_value=SyntheticSource()), self.assertRaises(contract.ContractError):
                    contract.main(["verify", "--source", str(root), "--manifest", str(baseline)])

    def test_drift_keeps_review_report_even_when_extractor_cannot_read_new_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            baseline = root / "baseline.json"
            baseline.write_bytes(contract.canonical(generate()))
            report, markdown = root / "report.json", root / "report.md"
            with patch.object(contract, "Source", side_effect=contract.ContractError("new upstream grammar")), contextlib.redirect_stdout(io.StringIO()):
                result = contract.main(["drift", "--manifest", str(baseline), "--source", str(root),
                    "--revision", "b" * 40, "--output", str(report), "--markdown", str(markdown)])
            self.assertEqual(result, 2)
            self.assertEqual(json.loads(report.read_bytes())["extraction_error"], "new upstream grammar")
            self.assertIn("manual review", markdown.read_text(encoding="utf-8"))

    def test_source_reads_committed_bytes_not_modified_checkout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            contract.git(root, "init", "--quiet")
            (root / "asset.txt").write_bytes(b"committed\n")
            contract.git(root, "-c", "core.autocrlf=false", "add", "asset.txt")
            contract.git(root, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "fixture")
            revision = contract.git(root, "rev-parse", "HEAD").decode().strip()
            (root / "asset.txt").write_bytes(b"dirty\r\n")
            source = contract.Source(root, revision)
            self.assertEqual(source.read("asset.txt"), b"committed\n")
            self.assertEqual(source.file_contracts()["asset.txt"]["sha256"], contract.digest(b"committed\n"))


if __name__ == "__main__":
    unittest.main()
