"""Exercise the rendered GPUI right-click menu through real input events."""

import argparse
from pathlib import Path

from editor_gdp import TEXT_MODIFIER, launch, visible_indicator


def open_menu(editor, target):
    editor.call("right_click", x=round(target[0]), y=round(target[1]))
    editor.call("bounds", id="document-context-menu")


def choose(editor, position):
    for _ in range(position):
        editor.press("down")
    editor.press("enter")


def check_clipboard(binary):
    with launch(binary) as editor:
        editor.call("input", text="bronze")
        editor.press(f"{TEXT_MODIFIER}-a")
        x, y, _, _ = editor.call("bounds", id="document-context-target")
        position = (x + 20, y + 15)
        open_menu(editor, position)
        choose(editor, 4)  # Undo, Redo, Cut, Copy.
        assert editor.document() == "bronze", editor.document()
        assert editor.call("clipboard") == "bronze"

        open_menu(editor, position)
        choose(editor, 3)
        assert editor.document() == "", editor.document()

        open_menu(editor, position)
        choose(editor, 3)  # Cut and Copy are now disabled.
        assert editor.document() == "bronze", editor.document()


def check_graph(binary):
    graph = {
        "name": "Materials",
        "data": {
            "alloy": {"id": 701, "nterm": "alloy"},
            "bronze": {"id": 701, "nterm": "alloy"},
        },
    }
    with launch(binary, thesaurus=graph) as editor:
        editor.type("Before\nbronze")
        editor.press("up")
        mark = visible_indicator(editor, (7, 13))["bounds"]
        open_menu(editor, (mark["x"] + mark["width"] / 2, mark["y"] + 2))
        choose(editor, 5)  # Undo, Redo, Paste, Select All, Edit alternatives.
        editor.call("bounds", id="kg-synonyms-panel")
        assert editor.document() == "Before\nbronze", editor.document()
        editor.click("kg-cancel-synonyms")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    binary = parser.parse_args().binary.resolve(strict=True)
    check_clipboard(binary)
    check_graph(binary)
    print("PASS right-click Cut, Copy, Paste, and Edit Alternatives on a knowledge graph term")


if __name__ == "__main__":
    main()
