import sys
import types
import unittest

import vey_adapter


class VeyAdapterTests(unittest.TestCase):
    def setUp(self):
        vey_adapter._RUNTIME = None

    def test_decision_keeps_sequence_and_certificate(self):
        calls = []
        runtime = object()

        class Certificate:
            def to_dict(self):
                return {"program": ["MAX target_alignment"]}

        def decide(**kwargs):
            calls.append(kwargs)
            return types.SimpleNamespace(
                answer="turn_left",
                decision_mode="structured",
                trust={"state": "confident"},
                certificate=Certificate(),
            )

        sys.modules["vey"] = types.SimpleNamespace(
            Runtime=lambda device: runtime,
            decide=decide,
        )
        request = {
            "schema": 1,
            "sequence": 9,
            "question": "Choose the action with the greatest target_alignment.",
            "state": "health=100",
            "candidates": {"wait": "target_alignment 0", "turn_left": "target_alignment 1"},
        }
        response = vey_adapter.decide(request)
        self.assertEqual(response["sequence"], 9)
        self.assertEqual(response["action"], "turn_left")
        self.assertEqual(response["certificate"]["program"], ["MAX target_alignment"])
        self.assertIs(calls[0]["runtime"], runtime)

    def test_rejects_actions_outside_the_candidate_set(self):
        sys.modules["vey"] = types.SimpleNamespace(
            Runtime=lambda device: object(),
            decide=lambda **kwargs: types.SimpleNamespace(
                answer="arbitrary_keypress",
                decision_mode="structured",
                trust={},
                certificate=None,
            ),
        )
        with self.assertRaisesRegex(RuntimeError, "outside the supplied candidates"):
            vey_adapter.decide({
                "schema": 1,
                "sequence": 1,
                "question": "Choose an action.",
                "candidates": {"wait": "target_alignment 0"},
            })

    def test_rejects_unversioned_or_incomplete_requests(self):
        with self.assertRaisesRegex(ValueError, "schema=1"):
            vey_adapter.decide({"schema": 2})
        with self.assertRaisesRegex(ValueError, "non-empty object"):
            vey_adapter.decide({
                "schema": 1,
                "sequence": 1,
                "question": "Choose an action.",
                "candidates": {},
            })


if __name__ == "__main__":
    unittest.main()
