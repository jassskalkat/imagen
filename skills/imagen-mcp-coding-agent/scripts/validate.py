#!/usr/bin/env python3
"""Validate the public Imagen coding-agent skill package."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path


SKILL_DIR = Path(__file__).resolve().parents[1]
SKILL_FILE = SKILL_DIR / "SKILL.md"
EVALS_FILE = SKILL_DIR / "evals" / "evals.json"


def fail(message: str) -> None:
    raise SystemExit(f"skill validation failed: {message}")


def main() -> None:
    if SKILL_DIR.name != "imagen-mcp-coding-agent":
        fail("skill folder name is incorrect")
    if not SKILL_FILE.is_file():
        fail("SKILL.md is missing")
    if (SKILL_DIR / "README.md").exists():
        fail("README.md must remain at repository level, not inside the skill folder")
    if not EVALS_FILE.is_file():
        fail("evals/evals.json is missing")

    text = SKILL_FILE.read_text(encoding="utf-8")
    parts = text.split("---", 2)
    if len(parts) != 3 or not parts[0].strip() == "":
        fail("SKILL.md must start with YAML frontmatter delimited by ---")
    frontmatter = parts[1]
    if "<" in frontmatter or ">" in frontmatter:
        fail("frontmatter contains forbidden XML angle brackets")
    name_match = re.search(r"(?im)^name:[ \t]*([a-z0-9-]+)[ \t]*$", frontmatter)
    if not name_match or name_match.group(1) != SKILL_DIR.name:
        fail("frontmatter name must match the skill folder")
    if any(word in name_match.group(1).lower() for word in ("claude", "anthropic")):
        fail("skill name uses a reserved vendor name")

    description_match = re.search(r"(?ims)^description:\s*(.+?)(?=^\w[\w-]*:|\Z)", frontmatter)
    if not description_match:
        fail("description is missing")
    description = " ".join(description_match.group(1).split())
    if len(description) >= 1024:
        fail("description is too long")
    if "use when" not in description.lower():
        fail("description must include trigger conditions")

    try:
        evals = json.loads(EVALS_FILE.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail(f"evals JSON is invalid: {exc}")
    if evals.get("skill_name") != SKILL_DIR.name:
        fail("evals skill_name must match the skill folder")
    if not evals.get("evals") or not evals.get("functional_evals"):
        fail("positive/negative trigger evals and functional evals are required")
    if not any(item.get("should_trigger") is True for item in evals["evals"]):
        fail("at least one positive trigger eval is required")
    if not any(item.get("should_trigger") is False for item in evals["evals"]):
        fail("at least one negative trigger eval is required")

    print("Imagen skill validation passed.")


if __name__ == "__main__":
    main()
