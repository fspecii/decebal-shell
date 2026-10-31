"""The harness source must not claim acceptance or copy host secrets."""

from __future__ import annotations

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

_ROOT = Path(__file__).resolve().parents[1]
_FORBIDDEN = (
    "os.environ.copy",
    "t3_thread",
    "mcp:launch",
    "api.x.ai",
    "ANTHROPIC_API_KEY",
    "full_acceptance = True",
    '"full_acceptance": True',
    "'full_acceptance': True",
)


class SourcePolicyTests(unittest.TestCase):
    def test_owned_tree_has_no_forbidden_claims(self) -> None:
        files = list((_ROOT / "lab").glob("*.py"))
        files.append(_ROOT / "desktop-lab")
        files.extend((_ROOT / "schema").glob("*.json"))
        docs = _ROOT.parents[1] / "docs" / "desktop-lab.md"
        if docs.is_file():
            files.append(docs)
        for path in files:
            text = path.read_text(encoding="utf-8")
            for item in _FORBIDDEN:
                self.assertNotIn(item, text, f"{path} contains {item}")


if __name__ == "__main__":
    unittest.main()
