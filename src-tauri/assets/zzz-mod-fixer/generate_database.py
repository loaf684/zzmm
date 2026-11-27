#!/usr/bin/env python3
"""Generate `database.json` from ZZZ-Mod-Fixer's per-character hash data.

The upstream tool (github.com/Vonksdesu/ZZZ-Mod-Fixer, MIT, a continuation of
petrascyll's original) keeps one hand-maintained Python module per character
under `Source Codes/Assets/PlayerCharacterPYData/`. Each module returns a dict
`hash -> [(CommandClass, args), ...]`. That data - not the code - is what ZZMM
ships; this script flattens the dicts into a single JSON asset.

Command names are kept verbatim. Positional arguments are bound to the command's
parameter names (the table below is the port spec: it names every parameter the
Rust engine expects), so an upstream signature change surfaces here as an error
instead of silently shifting arguments.

Usage, either:

    # From a checkout of ZZZ-Mod-Fixer (recommended for review)
    python generate_database.py --source "C:/src/ZZZ-Mod-Fixer" --out database.json

    # Download the pinned commit from GitHub, then generate
    python generate_database.py --download --out database.json

The output is deterministic: hashes are sorted, `json` is written compact.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sys
import urllib.request
from pathlib import Path

REPO = "Vonksdesu/ZZZ-Mod-Fixer"
# Pinned so a rebuild is reproducible; bump deliberately, never implicitly.
REF = "0aa483d6abbe40ba83d7a9d4b59f24a43b987c35"
REF_DATE = "2026-11-21"
GAME_VERSION = "3.0"
DATA_DIR = "Source Codes/Assets/PlayerCharacterPYData"

# name -> parameter names, in declaration order. `*args` is marked with "*".
# Anything not listed here fails the run: a new upstream command must be ported
# (or explicitly classified as buffer-pass work) before the asset is regenerated.
SIGNATURES: dict[str, list[str]] = {
    "log": ["*text"],
    "update_hash": ["new_hash"],
    "comment_sections": [],
    "comment_commandlists": ["commandlist_title"],
    "remove_section": ["capture_content", "capture_position"],
    "remove_indexed_sections": [
        "capture_content",
        "capture_indexed_content",
        "capture_position",
    ],
    "capture_section": ["capture_content", "capture_position"],
    "create_new_section": ["section_content", "saved_position", "capture_position"],
    # Upstream declares fields `trg_indices` / `transfer_indexed_sections`, but
    # the data calls it with `src_indices` / `trg_indices`; either way its
    # execute() reads an attribute that does not exist. Kept verbatim (it is
    # reported, never executed) so the asset matches the upstream data.
    "transfer_indexed_sections": ["src_indices", "trg_indices"],
    "multiply_section_if_missing": ["equiv_hashes", "extra_title"],
    "add_ib_check_if_missing": [],
    "add_section_if_missing": ["equiv_hashes", "section_title", "section_content"],
    # Buffer-level commands: detected and reported by ZZMM v1, never executed.
    "zzz_13_remap_texcoord": ["id", "old_format", "new_format"],
    "zzz_12_shrink_texcoord_color": ["id"],
    "update_buffer_blend_indices": ["hash", "old_indices", "new_indices"],
}

# Commands that rewrite `.buf` files. ZZMM v1 does not implement the buffer
# pass; the engine reports these instead of half-applying them.
BUFFER_COMMANDS = {
    "zzz_13_remap_texcoord",
    "zzz_12_shrink_texcoord_color",
    "update_buffer_blend_indices",
}


class Command:
    """Stand-in for the engine's command classes: only the name matters."""

    def __init__(self, name: str) -> None:
        self.name = name

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return self.name


def bind(command: str, args) -> dict:
    """Bind one `(CommandClass, args)` payload to a named JSON object."""
    params = SIGNATURES[command]
    varargs = bool(params) and params[0].startswith("*")
    vararg_name = params[0].lstrip("*") if varargs else ""

    if isinstance(args, dict):
        unknown = set(args) - set(params)
        if unknown:
            raise SystemExit(f"{command}: unknown parameter(s) {sorted(unknown)}")
        if varargs:
            raise SystemExit(f"{command}: keyword form is not supported for *args")
        bound = {name: None for name in params}
        bound.update(args)
        return bound

    positional = list(args)
    if varargs:
        return {vararg_name: [_plain(value) for value in positional]}

    if len(positional) > len(params):
        raise SystemExit(f"{command}: {len(positional)} args for {params}")

    bound = {name: None for name in params}
    for name, value in zip(params, positional):
        bound[name] = value
    return {name: _plain(value) for name, value in bound.items()}


def _plain(value):
    """Normalize dataclass-free values: tuples become lists, Command becomes name."""
    if isinstance(value, Command):
        return value.name
    if isinstance(value, (list, tuple)):
        return [_plain(item) for item in value]
    return value


def normalize_commands(raw, where: str) -> list[dict]:
    commands = []
    for entry in raw:
        if not isinstance(entry, (list, tuple)) or not entry:
            raise SystemExit(f"{where}: malformed command entry {entry!r}")
        klass, *rest = entry
        if not isinstance(klass, Command):
            raise SystemExit(f"{where}: command {klass!r} is not a command class")
        if klass.name not in SIGNATURES:
            raise SystemExit(f"{where}: unknown command {klass.name!r}")
        args = rest[0] if rest else ()
        commands.append({"cmd": klass.name, "args": bind(klass.name, args)})
    return commands


def load_module(path: Path):
    spec = importlib.util.spec_from_file_location(f"zzzfixer_{path.stem}", path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def parse_module(path: Path, character: str) -> dict:
    """Run one character module and return {hash: {character, label, commands}}."""
    module = load_module(path)
    if not hasattr(module, "get_hash_commands"):
        raise SystemExit(f"{path.name}: missing get_hash_commands()")

    classes = {name: Command(name) for name in SIGNATURES}
    raw = module.get_hash_commands(**classes)

    out = {}
    for hash_id, entry in raw.items():
        where = f"{character}:{hash_id}"
        if not re.fullmatch(r"[a-fA-F0-9]+", hash_id):
            raise SystemExit(f"{where}: hash is not hex")
        commands = normalize_commands(entry, where)
        label = next(
            (" - ".join(part for part in c["args"]["text"] if part)
             for c in commands if c["cmd"] == "log"),
            f"{character} {hash_id}",
        )
        out[hash_id.lower()] = {
            "character": character,
            "label": label,
            "commands": commands,
        }
    return out


def download(dest: Path) -> Path:
    """Fetch the pinned commit's character modules into `dest`."""
    api = f"https://api.github.com/repos/{REPO}/git/trees/{REF}?recursive=1"
    with urllib.request.urlopen(api) as response:
        tree = json.load(response)

    wanted = [
        node["path"]
        for node in tree["tree"]
        if node["path"].startswith(DATA_DIR + "/") and node["path"].endswith(".py")
    ]
    if len(wanted) < 80:
        raise SystemExit(f"only {len(wanted)} data files found at {REF}")

    for path in sorted(wanted):
        name = path.rsplit("/", 1)[1]
        if name == "__init__.py":
            continue
        url = f"https://raw.githubusercontent.com/{REPO}/{REF}/{path.replace(' ', '%20')}"
        with urllib.request.urlopen(url) as response:
            body = response.read()
        (dest / name).write_bytes(body)

    return dest


def generate(source: Path, out: Path) -> None:
    data_dir = source / DATA_DIR
    if not data_dir.is_dir():
        data_dir = source
    modules = sorted(p for p in data_dir.glob("*.py") if p.name != "__init__.py")
    if not modules:
        raise SystemExit(f"no character modules under {data_dir}")

    entries: dict[str, dict] = {}
    collisions: dict[str, list[str]] = {}
    histogram: dict[str, int] = {}

    for path in modules:
        character = path.stem
        for hash_id, entry in parse_module(path, character).items():
            if hash_id in entries:
                collisions.setdefault(hash_id, [entries[hash_id]["character"]]).append(
                    character
                )
            entries[hash_id] = entry
            for command in entry["commands"]:
                histogram[command["cmd"]] = histogram.get(command["cmd"], 0) + 1

    unknown = sorted(set(histogram) - set(SIGNATURES))
    if unknown:
        raise SystemExit(f"unported commands in the data: {unknown}")

    ordered = {hash_id: entries[hash_id] for hash_id in sorted(entries)}
    document = {
        "source": {
            "repo": f"https://github.com/{REPO}",
            "ref": REF,
            "refDate": REF_DATE,
            "gameVersion": GAME_VERSION,
            "dataDir": DATA_DIR,
            "license": "MIT (Copyright (c) 2026 VonksDesuuu; original by petrascyll)",
        },
        "characters": len(modules),
        "hashCount": len(ordered),
        "commands": {"used": sorted(histogram), "bufferPass": sorted(BUFFER_COMMANDS)},
        "hashCommands": ordered,
    }

    out.write_text(
        json.dumps(document, indent=1, sort_keys=False, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )

    print(f"modules:    {len(modules)}")
    print(f"hashes:     {len(ordered)}")
    print(f"size:       {out.stat().st_size / (1024 * 1024):.2f} MiB")
    print(f"commands:   {dict(sorted(histogram.items()))}")
    if collisions:
        # Shared hashes (feet/face/skin textures are reused across characters).
        # Upstream merges with `dict.update`, so the last module in sorted order
        # wins; ZZMM ships the merged result as-is.
        print(f"collisions: {len(collisions)} hashes defined by more than one character")
        for hash_id, characters in sorted(collisions.items())[:5]:
            print(f"  {hash_id}: {' -> '.join(characters)}")
        if len(collisions) > 5:
            print(f"  … {len(collisions) - 5} more")


def main() -> None:
    parser = argparse.ArgumentParser(prog="generate_database.py")
    parser.add_argument("--source", type=Path, default=None,
                        help=f"checkout of {REPO} (contains '{DATA_DIR}')")
    parser.add_argument("--download", action="store_true",
                        help=f"fetch {REPO}@{REF[:8]} from GitHub into a temp dir")
    parser.add_argument("--out", type=Path, default=Path(__file__).with_name("database.json"))
    args = parser.parse_args()

    if args.source:
        generate(args.source, args.out)
    elif args.download:
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            modules = download(Path(tmp))
            generate(modules, args.out)
    else:
        parser.error("pass --source <checkout> or --download")


if __name__ == "__main__":
    sys.exit(main())
