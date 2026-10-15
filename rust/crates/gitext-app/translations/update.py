#!/usr/bin/env python3
"""Regenerates the translation catalogs (<code>.json) of the Rust port.

Reads the English text of every tr("…"), trf("…", …) and n_("…") literal in
crates/gitext-app/src, looks it up in upstream's translation files (the .xlf files of the
.NET application, read from git history), and writes, for each language, the strings that
have a translation. Strings without one are shown in English.

Run from anywhere inside the repository after adding or changing UI strings:

    python3 rust/crates/gitext-app/translations/update.py

The `catalogs_match_the_sources` test fails while a catalog holds strings the sources no
longer show.
"""

import json
import os
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from collections import Counter

# the last commit with the .NET sources
UPSTREAM = "16461a8^1"
XLF_DIR = "src/app/GitUI/Translation"

# catalog code -> upstream translation file (keep in step with LANGUAGES in src/i18n.rs)
LANGUAGES = {
    "cs": "Czech",
    "nl": "Dutch",
    "fi": "fi",
    "fr": "French",
    "de": "German",
    "hr": "hr",
    "hu": "hu",
    "id": "Indonesian",
    "it": "Italian",
    "ja": "Japanese",
    "ko": "Korean",
    "lv": "Latvian",
    "no": "no",
    "pl": "Polish",
    "pt-PT": "Portuguese",
    "pt-BR": "Portuguese (Brazil)",
    "ro": "Romanian",
    "ru": "Russian",
    "sr": "sr",
    "zh-CN": "Simplified Chinese",
    "es": "Spanish",
    "es-AR": "Spanish (Argentina)",
    "sv": "sv",
    "th": "th",
    "zh-TW": "Traditional Chinese",
    "tr": "Turkish",
    "uk": "uk",
    "vi": "vi",
}

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(os.path.dirname(HERE), "src")

LITERAL = re.compile(r'\b(?:tr|trf|n_)\(\s*"((?:[^"\\]|\\.)*)"')
PLACEHOLDER = re.compile(r"\{\d+\}")
# "&File" mnemonics, and the "(&F)" suffixes of the Asian translations
ACCEL_SUFFIX = re.compile(r"\s*[(（]&\w[)）]")


def unescape(s):
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            n = s[i + 1]
            out.append({"n": "\n", "t": "\t", '"': '"', "\\": "\\", "'": "'"}.get(n, "\\" + n))
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def marked_strings():
    found = set()
    for root, _, files in os.walk(SRC):
        for f in files:
            if f.endswith(".rs") and f != "i18n.rs":
                with open(os.path.join(root, f), encoding="utf-8") as fh:
                    for m in LITERAL.finditer(fh.read()):
                        found.add(unescape(m.group(1)))
    return found


def strip_mnemonic(s):
    s = ACCEL_SUFFIX.sub("", s)
    return s.replace("&&", "\0").replace("&", "").replace("\0", "&").strip()


def read_xlf(name):
    data = subprocess.run(["git", "show", f"{UPSTREAM}:{XLF_DIR}/{name}.xlf"], cwd=HERE, check=True, capture_output=True).stdout
    votes = {}
    for unit in ET.fromstring(data).iter("trans-unit"):
        source = unit.findtext("source") or ""
        target = unit.findtext("target") or ""
        source, target = strip_mnemonic(source), strip_mnemonic(target)
        if not source or not target:
            continue
        votes.setdefault(source, Counter())[target] += 1
    # the most common translation of each source text ("Commit" is a noun more often than a
    # verb); one kept in English stays in the table, so that no looser match replaces it
    return {s: c.most_common(1)[0][0] for s, c in votes.items()}


ELLIPSES = ("...", "…")
COLONS = (":", "：")


def split_suffix(s):
    """Splits a trailing ellipsis or colon off a string."""
    for suffix in ELLIPSES + COLONS:
        if s.endswith(suffix):
            return s[: -len(suffix)].rstrip(), suffix
    return s, ""


def match_case(target, text):
    """Gives the translation the case of the first letter of the port's text."""
    if not target or not text[0].isalpha():
        return target
    first = target[0].lower() if text[0].islower() else target[0].upper()
    return first + target[1:]


def translate(text, table, folded):
    if text in table:
        return table[text]
    # the port writes "…" where upstream wrote "...", drops or adds a label's colon, and
    # sometimes capitalizes differently
    base, suffix = split_suffix(text)
    for candidate in [base] + [base + s for s in ELLIPSES + COLONS]:
        if candidate != text and candidate in table:
            target, _ = split_suffix(table[candidate])
            return target + suffix
    for candidate in [base] + [base + s for s in ELLIPSES + COLONS]:
        if candidate.lower() in folded:
            target, _ = split_suffix(table[folded[candidate.lower()]])
            return match_case(target, text) + suffix
    return None


def main():
    strings = sorted(marked_strings())
    print(f"{len(strings)} strings marked for translation")
    for code, name in LANGUAGES.items():
        table = read_xlf(name)
        folded = {}
        for source in sorted(table):
            folded.setdefault(source.lower(), source)
        catalog = {}
        for text in strings:
            target = translate(text, table, folded)
            if target and target != text and sorted(set(PLACEHOLDER.findall(target))) == sorted(set(PLACEHOLDER.findall(text))):
                catalog[text] = target
        with open(os.path.join(HERE, f"{code}.json"), "w", encoding="utf-8", newline="\n") as fh:
            json.dump(catalog, fh, ensure_ascii=False, indent=1, sort_keys=True)
            fh.write("\n")
        print(f"{code:6} {len(catalog)} translated")


if __name__ == "__main__":
    sys.exit(main())
