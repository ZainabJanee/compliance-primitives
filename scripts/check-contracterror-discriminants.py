#!/usr/bin/env python3
"""Fail if any contract uses duplicate #[repr(u32)] discriminant numbers."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONTRACTS_DIR = ROOT / "contracts"

pattern = re.compile(r"pub\s+enum\s+Error\s*\{(.*?)\}", re.DOTALL)
value_pattern = re.compile(r"\b(\w+)\s*=\s*(\d+)\b")

issues: list[str] = []
count = 0

for path in sorted(CONTRACTS_DIR.glob("*/src/lib.rs")):
    text = path.read_text(encoding="utf-8")
    if "contracterror" not in text:
        continue

    match = pattern.search(text)
    if match is None:
        continue

    count += 1
    seen: dict[int, str] = {}
    for name, raw_value in value_pattern.findall(match.group(1)):
        value = int(raw_value)
        if value in seen:
            issues.append(
                f"{path.relative_to(ROOT)}: {seen[value]} and {name} both use discriminant {value}"
            )
            continue
        seen[value] = name

if issues:
    print("Duplicate #[contracterror] discriminants found:")
    for issue in issues:
        print(f"- {issue}")
    print(f"\nFound {len(issues)} duplicate(s) across {count} Error enums.")
    sys.exit(1)

print(f"Checked {count} Error enums. No duplicate discriminant values found.")
