#!/usr/bin/env python3
"""Regenerate the shipped outdated-hash table from a ZZZ-Mod-Fixer checkout.

    python scripts/generate-fixer-hash-map.py <path-to-ZZZ-Mod-Fixer> [--out FILE]

The table maps an *outdated* 3DMigoto `hash = ...` value to the value the
current game version uses, so ZZMM can repair mods written for older patches.

Where the pairs come from
-------------------------
Upstream keeps one Python module per character in
`Source Codes/Assets/PlayerCharacterPYData/`. Each module is a dictionary keyed
by a hash whose command list may contain `update_hash(new_hash)` — that is
upstream's own statement of "this hash became that hash". Only those renames are
extracted; no upstream code, section template or command is copied.

Safety rules applied here
-------------------------
Upstream merges every character module into one global table and only warns
about collisions. That is unsafe for a rewriter: a hash that is current for one
character can be an outdated value for another (e.g. `ebac056e` is the current
2048p NormalMap hash for most characters, but Belle-Temple/Wise-Temple rename it
to `798adba3`), so a global merge would rewrite hashes that are still correct.
This generator therefore drops:

1. chains that leave a module (renames are followed to a fixpoint *inside one
   character module* only),
2. old hashes that map to more than one target anywhere,
3. old hashes that are the target of another pair (ambiguous round trips),
4. old hashes that the current game database still lists (the hash is not
   outdated, so "fixing" it would corrupt a working mod).

Every dropped pair is a false negative, never a false positive: the table only
claims a rewrite when the value is provably stale. Counts are written into the
table's `meta` block.
"""

from __future__ import annotations

import argparse
import ast
import datetime
import json
import os
import subprocess
import sys
from pathlib import Path

CHARACTER_DIR = Path("Source Codes") / "Assets" / "PlayerCharacterPYData"
DATABASE_DIR = Path("Source Codes") / "ZZZHashIDCharactersDatabase"
RESOLUTIONS = ("1024p", "2048p")
HASH_FIELDS = ("draw_vb", "position_vb", "texcoord_vb", "blend_vb", "ib")

COMMANDS_THAT_RENAME = "update_hash"


def is_hash(value: object) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 8
        and all(character in "0123456789abcdefABCDEF" for character in value)
    )


def parse_character_module(path: Path) -> dict[str, set[str]]:
    """`{old_hash: {new_hash, ...}}` from one upstream character module."""
    tree = ast.parse(path.read_text(encoding="utf-8"))
    table = None
    for node in ast.walk(tree):
        if not isinstance(node, ast.Dict):
            continue
        if not any(isinstance(key, ast.Constant) and is_hash(key.value) for key in node.keys):
            continue
        if table is None or len(node.keys) > len(table.keys):
            table = node
    if table is None:
        return {}

    renames: dict[str, set[str]] = {}
    for key, value in zip(table.keys, table.values):
        old = ast.literal_eval(key)
        if not is_hash(old) or not isinstance(value, ast.List):
            continue
        for command in value.elts:
            if not isinstance(command, ast.Tuple) or not command.elts:
                continue
            head = command.elts[0]
            if not isinstance(head, ast.Name) or head.id != COMMANDS_THAT_RENAME:
                continue
            if len(command.elts) < 2 or not isinstance(command.elts[1], ast.Tuple):
                continue
            if not command.elts[1].elts:
                continue
            new = ast.literal_eval(command.elts[1].elts[0])
            if is_hash(new):
                renames.setdefault(old.lower(), set()).add(new.lower())
    return renames


def follow(name: str, edges: dict[str, set[str]]) -> str:
    """Walk one module's renames to a fixpoint; stop at any ambiguity."""
    seen = {name}
    current = name
    while True:
        candidates = edges.get(current)
        if not candidates or len(candidates) != 1:
            return current
        following = next(iter(candidates))
        if following in seen:
            return current
        seen.add(following)
        current = following


def current_hashes(checkout: Path) -> tuple[set[str], str]:
    """Every hash the newest bundled game database still lists."""
    versions = sorted(
        path
        for path in (checkout / DATABASE_DIR / "2048p").glob("Database_*")
        if path.is_dir()
    )
    if not versions:
        raise SystemExit(f"no Database_* folder under {checkout / DATABASE_DIR}")

    version = versions[-1]
    wanted = f"{version.name.split('_', 1)[1]}"
    found: set[str] = set()

    for resolution in RESOLUTIONS:
        for path in sorted((checkout / DATABASE_DIR / resolution / version.name).glob("*.json")):
            for component in json.loads(path.read_text(encoding="utf-8")):
                for field in HASH_FIELDS:
                    value = component.get(field)
                    if is_hash(value):
                        found.add(value.lower())
                for group in component.get("texture_hashes", []):
                    for texture in group:
                        if len(texture) >= 3 and is_hash(texture[2]):
                            found.add(texture[2].lower())

    return found, wanted


def source_commit(checkout: Path) -> str:
    try:
        return subprocess.run(
            ["git", "-C", str(checkout), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("checkout", type=Path, help="path to a Vonksdesu/ZZZ-Mod-Fixer checkout")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "src-tauri" / "src" / "mods" / "fixer_hash_map.json",
    )
    args = parser.parse_args()

    characters = args.checkout / CHARACTER_DIR
    if not characters.is_dir():
        print(f"error: {characters} not found", file=sys.stderr)
        return 2

    pairs: dict[str, set[str]] = {}
    files = 0
    for module in sorted(characters.glob("*.py")):
        if module.name == "__init__.py":
            continue
        edges = parse_character_module(module)
        if not edges:
            continue
        files += 1
        for old in edges:
            target = follow(old, edges)
            if target != old:
                pairs.setdefault(old, set()).add(target)

    raw = sum(len(targets) for targets in pairs.values())
    conflicts = {old for old, targets in pairs.items() if len(targets) > 1}
    unique = {old: next(iter(targets)) for old, targets in pairs.items() if len(targets) == 1}
    targets = set(unique.values())
    ambiguous = {old for old in unique if old in targets}

    known, version = current_hashes(args.checkout)
    still_current = {old for old in unique if old in known}

    renames = {
        old: new
        for old, new in sorted(unique.items())
        if old not in ambiguous and old not in still_current
    }

    table = {
        "meta": {
            "targetGameVersion": version,
            "source": "Vonksdesu/ZZZ-Mod-Fixer",
            "sourceUrl": "https://github.com/Vonksdesu/ZZZ-Mod-Fixer",
            "sourceCommit": source_commit(args.checkout),
            "sourceLicense": "GPL-3.0 (upstream code and data). This file is a derived list of hash pairs, not upstream source.",
            "generated": datetime.date.today().isoformat(),
            "characterModules": files,
            "renames": len(renames),
            "dropped": {
                "conflictingTargets": len(conflicts),
                "ambiguousRoundTrip": len(ambiguous),
                "stillCurrent": len(still_current),
                "selfRename": raw - len(pairs),
            },
        },
        "renames": renames,
    }

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(table, indent=2, sort_keys=False) + "\n", encoding="utf-8")
    print(f"{args.out}: {len(renames)} renames for game {version} (dropped {len(conflicts)} conflicting, "
          f"{len(ambiguous)} ambiguous, {len(still_current)} still current)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
