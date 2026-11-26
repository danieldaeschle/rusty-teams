#!/usr/bin/env python3
"""Moves literal text sizes in the views to the type tokens in `app_base::theme`.

Sizes without a token stay literal.

Safe to run again. After rebasing the theming branch, drop the generated commit, run this
script, then run `cargo fmt` and commit the result:

    python3 scripts/apply-type-tokens.py
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES = {"app": "crate::theme", "app-ui-kit": "app_base::theme"}

# Sizes tied to a fixed shape stay literal.
SKIPPED_FILES = {
    "crates/app/src/views/avatar.rs",
    "crates/app/src/views/call_layouts.rs",
    "crates/app/src/views/rail.rs",
    "crates/app-ui-kit/src/widgets.rs",
}

TOKENS = {
    10.5: "text_caption",
    11.0: "text_caption",
    11.5: "text_caption",
    12.0: "text_small",
    12.5: "text_small",
    13.0: "text_label",
    13.5: "text_label",
    14.0: "text_body",
    16.0: "text_heading",
    20.0: "text_title",
}

CALL = re.compile(r"\.text_size\(px\((\d+(?:\.\d*)?)\)\)")


def replacement(match):
    token = TOKENS.get(float(match[1]))
    return f".text_size(theme::{token}())" if token else match[0]


def convert(text):
    return CALL.sub(replacement, text)


def split_tests(text):
    """Test modules assert exact sizes, so only the code before them is converted."""
    marker = text.find("#[cfg(test)]\nmod ")
    return (text, "") if marker < 0 else (text[:marker], text[marker:])


def with_import(text, path):
    if re.search(r"^\s*use [^;]*\btheme\b[^;]*;", text, re.M):
        return text
    first_use = re.search(r"^use ", text, re.M)
    line = f"use {path};\n"
    if not first_use:
        return line + "\n" + text
    return text[: first_use.start()] + line + text[first_use.start() :]


def main():
    changed = 0
    for crate, import_path in CRATES.items():
        for file in sorted((ROOT / "crates" / crate / "src").rglob("*.rs")):
            relative = file.relative_to(ROOT).as_posix()
            if relative in SKIPPED_FILES:
                continue
            original = file.read_text()
            code, tests = split_tests(original)
            converted = convert(code)
            if converted == code:
                continue
            file.write_text(with_import(converted, import_path) + tests)
            changed += 1
    print(f"converted {changed} files")


if __name__ == "__main__":
    main()
