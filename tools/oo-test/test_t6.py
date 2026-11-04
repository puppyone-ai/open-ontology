import json
from pathlib import Path
import subprocess
import tempfile
import time
import types
import unittest

import t6


class GoldDialogueTests(unittest.TestCase):
    def test_numeric_comparison_is_lossless_and_ordered(self):
        value = "12345678901234567890123456789012345678.1200"
        self.assertEqual(t6.cell(value), value[:-2])
        self.assertTrue(t6.classify({"rows": [["1.00"]]}, [["1"]], queried=True)["equal"])
        self.assertFalse(t6.classify({"rows": [["2"], ["1"]]}, [["1"], ["2"]],
                                     queried=True)["equal"])
        self.assertFalse(t6.classify({"rows": [[1.5]]}, [["1.5"]], queried=True)["equal"])

    def test_null_is_not_zero_or_text(self):
        for value in (0, "0", "NULL", ""):
            report = t6.classify({"rows": [[value]]}, [[None]], queried=True)
            self.assertFalse(report["equal"])
        report = t6.classify({"rows": [[None]]}, [[None]], queried=True)
        self.assertTrue(report["equal"])
        self.assertTrue(report["null"])

    def test_truncation_refusal_and_unverified_never_pass(self):
        for flags, status in [({"truncated": True}, "truncated"),
                              ({"refused": True}, "refused"), ({}, "unverified")]:
            report = t6.classify({"rows": [["1"]]}, [["1"]], **flags)
            self.assertEqual(report["status"], status)
            self.assertFalse(report["equal"])

    def test_no_shell_or_model_selected_connection(self):
        self.assertEqual(t6.cli_arguments({"command": "query", "argument": "SELECT 1"}),
                         ["query", "SELECT 1"])
        for value in [
            {"command": "query", "argument": "--token-file=/tmp/a"},
            {"command": "whoami", "argument": "--gateway=evil"},
            {"command": "tables", "gateway": "evil"},
            {"command": "sh", "argument": "cat secret"},
            {"command": "query", "argument": "a\x00b"},
            {"command": "login", "argument": "not-a-code"},
        ]:
            with self.assertRaises(ValueError):
                t6.cli_arguments(value)

    def test_binding_requires_real_success_and_redacts_material(self):
        code = "OO-ABCD-EFGH"
        def response(command, argument=""):
            return {"choices": [{"message": {"role": "assistant", "tool_calls": [{
                "id": command, "function": {"name": "oo", "arguments":
                    json.dumps({"command": command, "argument": argument})}}]}}]}
        responses = iter([response("login", code), response("whoami")])
        model = types.SimpleNamespace(call=lambda _: next(responses))
        with tempfile.TemporaryDirectory() as root:
            prompt = t6.bind_agent(model, lambda _: {"exit": 0, "body": {}},
                                   "Bind " + code, Path(root))
            self.assertNotIn(code, prompt)
            self.assertTrue(json.loads((Path(root) / "binding.json").read_text())["verified"])
        model.call = lambda _: response("login", code)
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(ValueError):
                t6.bind_agent(model, lambda _: {"exit": 4, "body": {}}, code, Path(root))
            self.assertFalse(json.loads((Path(root) / "binding.json").read_text())["verified"])

    def test_budget_rejects_before_egress(self):
        model = t6.Model.__new__(t6.Model)
        model.deadline = time.monotonic() + 100
        model.calls, model.reserved = t6.MAX_CALLS, 0
        with self.assertRaisesRegex(ValueError, "MODEL_CALL_LIMIT"):
            model.call([])
        model.calls = 0
        with self.assertRaisesRegex(ValueError, "MODEL_REQUEST_BUDGET"):
            model.call([{"content": "x" * t6.MAX_REQUEST_BYTES}])
        self.assertEqual(model.calls, 0)

    def test_model_receives_question_not_gold(self):
        seen = []
        responses = iter([
            {"choices": [{"message": {"role": "assistant", "tool_calls": [{
                "id": "test", "type": "function", "function": {"name": "oo", "arguments":
                    json.dumps({"command": "query", "argument": "SELECT 1"})}}]}}]},
            {"choices": [{"message": {"role": "assistant", "content": '{"rows":[["1"]]}'}}]},
        ])
        def call(messages):
            seen.append(json.dumps(messages))
            return next(responses)
        model = types.SimpleNamespace(call=call, module=types.SimpleNamespace(Refusal=RuntimeError))
        with tempfile.TemporaryDirectory() as root:
            report = t6.dialogue(model, lambda args: {"exit": 0, "body": {"rows": [["1"]]}},
                                 "share prompt", {"id": "Q01", "question": "Count?"},
                                 [["gold-never-sent"]], Path(root))
        self.assertEqual(report["status"], "mismatch")
        self.assertNotIn("gold-never-sent", "".join(seen))

    def test_entrypoint_requires_opt_in_before_resources(self):
        entry = Path(__file__).with_name("oo-test")
        for arguments, expected in [(["t6", "--help"], 0),
                                    (["t6", "--run-id", "../escape", "--execute"], 2),
                                    (["t6", "--run-id", "test-no-opt-in"], 2)]:
            result = subprocess.run([str(entry), *arguments], capture_output=True)
            self.assertEqual(result.returncode, expected)


if __name__ == "__main__":
    unittest.main()
