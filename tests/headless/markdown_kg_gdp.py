"""Verify Markdown-backed role edits with the editor's GDP driver."""

import argparse
from pathlib import Path
import tempfile

from editor_gdp import TEXT_MODIFIER, launch, visible_indicator


CONCEPT = "copper alloy"
OLD_SYNONYM = "bronze"
NEW_SYNONYM = "bell metal"
SOURCE = (
    "# Copper Alloy\n\n"
    "synonyms:: bronze\n"
    "priority:: 42\n"
    "pinned:: true\n\n"
    "An alloy of copper. See [[aluminium]] for a separate concept.\n"
)
OTHER_SOURCE = "# Aluminium\n\nsynonyms:: aluminum\n"


def enter_document(editor, text):
    editor.press(f"{TEXT_MODIFIER}-a")
    if text:
        editor.type(text)
    else:
        editor.press("backspace")
    assert editor.document() == text, repr(editor.document())


def place_caret_in_term(editor):
    editor.press("home")
    editor.press("right")


def open_synonyms(editor, draft):
    place_caret_in_term(editor)
    editor.press("ctrl-shift-a")
    editor.call("bounds", id="kg-synonyms-panel")
    editor.press(f"{TEXT_MODIFIER}-a")
    editor.type(draft)


def check_autocomplete(editor, prefix, expected=None):
    # Feed input as keystrokes so autocomplete runs as it does during typing.
    enter_document(editor, "")
    editor.call("input", text=prefix)
    editor.advance()
    editor.press("tab")
    if expected is None:
        assert editor.document().rstrip() == prefix, repr(editor.document())
    else:
        assert editor.document() == expected, repr(editor.document())


def check_markdown_role(binary, directory):
    concept_file = directory / f"{CONCEPT}.md"
    concept_file.write_text(SOURCE, encoding="utf-8")
    other_file = directory / "aluminium.md"
    other_file.write_text(OTHER_SOURCE, encoding="utf-8")
    launch_args = ["--kg-dir", str(directory)]

    with launch(binary, extra_args=launch_args) as editor:
        check_autocomplete(editor, "bro", OLD_SYNONYM)
        enter_document(editor, CONCEPT)
        place_caret_in_term(editor)
        editor.press("alt-down")
        assert editor.document() == OLD_SYNONYM
        enter_document(editor, CONCEPT)
        initial = visible_indicator(editor, (0, len(CONCEPT)))

        open_synonyms(editor, NEW_SYNONYM)
        assert editor.document() == CONCEPT, "Editing the KG draft modified the document"
        editor.click("kg-save-synonyms")
        assert editor.document() == CONCEPT, "Saving synonyms modified the document"
        assert concept_file.read_text(encoding="utf-8") == SOURCE.replace(
            f"synonyms:: {OLD_SYNONYM}", f"synonyms:: {NEW_SYNONYM}"
        )
        assert other_file.read_text(encoding="utf-8") == OTHER_SOURCE

        place_caret_in_term(editor)
        editor.press("alt-down")
        assert editor.document() == NEW_SYNONYM, repr(editor.document())
        rebuilt = visible_indicator(editor, (0, len(NEW_SYNONYM)))
        assert initial["active"] != rebuilt["active"]
        check_autocomplete(editor, "bell", NEW_SYNONYM)
        check_autocomplete(editor, "bro")

    with launch(binary, extra_args=launch_args) as editor:
        check_autocomplete(editor, "bell", NEW_SYNONYM)
        enter_document(editor, CONCEPT)
        place_caret_in_term(editor)
        editor.press("alt-down")
        assert editor.document() == NEW_SYNONYM
        open_synonyms(editor, "uncommitted alias")
        saved = concept_file.read_bytes()
        external = saved + b"\nAn edit from another process.\n"
        concept_file.write_bytes(external)
        editor.click("kg-save-synonyms")
        editor.call("bounds", id="kg-synonyms-panel")
        assert concept_file.read_bytes() == external
        assert editor.document() == NEW_SYNONYM
        editor.click("kg-cancel-synonyms")
    print("PASS source Markdown edit, live autocomplete and cycling, reopen, and stale-write protection")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="Terraphim Editor built with --features gdp")
    binary = parser.parse_args().binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="terraphim-markdown-kg-") as directory:
        check_markdown_role(binary, Path(directory))


if __name__ == "__main__":
    main()
