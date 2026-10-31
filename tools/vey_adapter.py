"""JSON-lines adapter between DOASCII and the Vey Python runtime.

Each input line must contain schema=1, a Vey decision question, structured
state, and candidate consequence descriptions keyed by action id. Output is a
single schema=1 decision line. Vey does not inspect Doom pixels or ASCII.
"""
from __future__ import annotations

from contextlib import redirect_stdout
import json
import sys
from typing import Any


MAX_LINE_BYTES = 1_048_576
_RUNTIME = None


def decide(request: Any) -> dict[str, Any]:
    if not isinstance(request, dict) or request.get("schema") != 1:
        raise ValueError("request must be an object with schema=1")
    question = request.get("question")
    candidates = request.get("candidates")
    state = request.get("state", "")
    if not isinstance(question, str) or not question.strip():
        raise ValueError("question must be a non-empty string")
    if not isinstance(candidates, dict) or not candidates:
        raise ValueError("candidates must be a non-empty object")
    if any(not isinstance(key, str) or not isinstance(value, str) or not value.strip()
           for key, value in candidates.items()):
        raise ValueError("candidate ids and descriptions must be non-empty strings")
    if not isinstance(state, str):
        raise ValueError("state must be a string")
    sequence = request.get("sequence")
    if not isinstance(sequence, int) or isinstance(sequence, bool) or sequence < 0:
        raise ValueError("sequence must be a non-negative integer")

    global _RUNTIME
    with redirect_stdout(sys.stderr):
        import vey

        if _RUNTIME is None:
            _RUNTIME = vey.Runtime(device="cpu")
        result = vey.decide(
            question=question,
            candidates=candidates,
            state=state or None,
            runtime=_RUNTIME,
            explain=True,
        )
    action = result.answer
    if action is not None and action not in candidates:
        raise RuntimeError("Vey returned an action outside the supplied candidates")
    certificate = getattr(result, "certificate", None)
    return {
        "schema": 1,
        "sequence": sequence,
        "action": action,
        "decision_mode": result.decision_mode,
        "trust": result.trust,
        "certificate": certificate.to_dict() if certificate is not None else None,
    }


def main() -> int:
    for line_number, line in enumerate(sys.stdin, start=1):
        if len(line.encode("utf-8")) > MAX_LINE_BYTES:
            response = {"schema": 1, "sequence": None, "error": "request line exceeds 1 MiB"}
        else:
            request = None
            try:
                request = json.loads(line)
                response = decide(request)
            except Exception as error:  # Keep the protocol alive; never invent an action.
                sequence = request.get("sequence") if isinstance(request, dict) else None
                response = {"schema": 1, "sequence": sequence, "error": f"line {line_number}: {error}"}
        sys.stdout.write(json.dumps(response, separators=(",", ":")) + "\n")
        sys.stdout.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
