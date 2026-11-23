"""Run Terraphim's three editor workflows via its opt-in headless driver."""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time

COMMAND_PACK = """---
name: search
usage: search <pattern>
parameters:
  - name: pattern
    required: true
---
# Search
"""
SAVED_DOCUMENT = "# Headless fixture\n\nEdit this stable paragraph.\n\nmachine learning stays here.\n"
UPDATED_PARAGRAPH = "The edited block survives save and undo."
TEXT_MODIFIER = "cmd" if sys.platform == "darwin" else "ctrl"
REDO = "cmd-shift-z" if sys.platform == "darwin" else "ctrl-y"


class Editor:
    def __init__(self, connection):
        self.stream = connection.makefile("rwb")

    def call(self, command, **arguments):
        message = json.dumps({"cmd": command, **arguments}).encode()
        self.stream.write(message + b"\n")
        self.stream.flush()
        line = self.stream.readline()
        assert line, f"Editor closed the socket during {command}"
        reply = json.loads(line)
        assert reply["ok"], f"{command} {arguments}: {reply.get('error', reply)}"
        return reply.get("result")

    def press(self, key):
        self.call("press", key=key)

    def type(self, text):
        self.call("type", text=text)

    def click(self, selector):
        self.call("click", selector=selector)

    def document(self):
        return self.call("document")

    def advance(self, milliseconds=300):
        self.call("advance", ms=milliseconds)

    def end_of_document(self):
        # Down and End work on both of our GPUI platforms.
        for _ in self.document().splitlines():
            self.press("down")
        self.press("end")


@contextmanager
def launch(binary, fixture=None, thesaurus=None, *, extra_args=()):
    # A short temporary path avoids the platform's Unix socket path limit.
    with tempfile.TemporaryDirectory(prefix="gdp-", dir="/tmp") as working_dir:
        directory = Path(working_dir)
        socket_path = directory / "gdp.sock"
        commands = directory / "commands"
        commands.mkdir()
        (commands / "search.md").write_text(COMMAND_PACK, encoding="utf-8")
        env = {
            **os.environ,
            "GDP_SOCKET": str(socket_path),
            "TERRAPHIM_COMMANDS_DIR": str(commands),
            "ODKD_SOCKET": "none",
        }
        args = [str(binary), *extra_args]
        if fixture:
            args += ["--file", str(fixture)]
        if isinstance(thesaurus, Path):
            args += ["--thesaurus", str(thesaurus)]
        elif thesaurus:
            graph = directory / "role.json"
            graph.write_text(json.dumps(thesaurus), encoding="utf-8")
            args += ["--thesaurus", str(graph)]
        log_path = directory / "editor.log"
        with log_path.open("w", encoding="utf-8") as log:
            process = subprocess.Popen(args, env=env, stdout=log, stderr=log)
            connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            connection.settimeout(10)
            try:
                wait_for_connection(connection, socket_path, process)
                editor = Editor(connection)
                try:
                    yield editor
                    editor.call("quit")
                    assert process.wait(timeout=10) == 0
                finally:
                    editor.stream.close()
            except Exception:
                print(log_path.read_text(encoding="utf-8"), file=sys.stderr)
                raise
            finally:
                connection.close()
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


def wait_for_connection(connection, socket_path, process):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Headless editor exited with code {process.returncode}")
        try:
            connection.connect(str(socket_path))
            return
        except (FileNotFoundError, ConnectionRefusedError):
            time.sleep(0.05)
    raise TimeoutError("Headless editor did not create a usable GDP socket")


def check_slash(binary):
    with launch(binary) as editor:
        for shortcut in ("se", "/se", "/sesearch "):
            for key in ("enter", "tab"):
                editor.press(f"{TEXT_MODIFIER}-a")
                editor.type(shortcut)
                editor.advance()
                editor.press(key)
                assert editor.document() == "/search ", (shortcut, key, editor.document())
    print("PASS se, /se, and /sesearch complete on Enter and Tab")


def check_blocks(binary, fixture):
    with launch(binary, fixture) as editor:
        original = editor.document()
        assert "Edit this stable paragraph." in original
        editor.press("ctrl-shift-b")
        editor.click("edit-block-1")
        editor.press(f"{TEXT_MODIFIER}-a")
        editor.type(UPDATED_PARAGRAPH)
        assert editor.document() == original, "The draft escaped into the document before Apply"
        editor.click("apply-block")
        modified = editor.document()
        assert UPDATED_PARAGRAPH in modified
        assert "Edit this stable paragraph." not in modified
        editor.press("ctrl-shift-b")
        editor.press(f"{TEXT_MODIFIER}-z")
        assert editor.document() == original
        editor.press(REDO)
        assert editor.document() == modified
        editor.end_of_document()
        editor.press("enter")
        editor.type("/save")
        editor.press("escape")
        editor.press("enter")
        editor.advance()
        saved = fixture.read_text(encoding="utf-8")
        assert UPDATED_PARAGRAPH in saved
        assert "machine learning stays here." in saved
        assert "/save" not in saved
    print("PASS editable Blocks, undo/redo, and save via the editor")



def visible_indicator(editor, span):
    results = editor.call("indicators")
    matches = [
        record
        for record in results
        if (record["start"], record["end"]) == span
        and record["count"] == 2
        and record["underline_count"] >= 1
        and record["bounds"]["width"] > 0
        and record["bounds"]["height"] > 0
    ]
    assert len(matches) == 1, (span, results)
    return matches[0]


def check_alternatives(binary):
    with launch(binary) as editor:
        editor.type("machine learning")
        editor.advance()
        original = visible_indicator(editor, (0, len("machine learning")))
        editor.press("home")
        editor.press("right")
        editor.press("alt-down")
        assert editor.document() == "ml"
        cycled = visible_indicator(editor, (0, len("ml")))
        assert cycled["active"] != original["active"], (original, cycled)
        editor.press("right")
        editor.press("alt-up")
        assert editor.document() == "machine learning"
        restored = visible_indicator(editor, (0, len("machine learning")))
        assert restored["active"] == original["active"]
    print(
        "PASS painted KG underline and two alternatives, active index "
        f'{original["active"]} -> {cycled["active"]} -> {restored["active"]}'
    )

def check_knowledge_graph(binary):
    graph = {
        "name": "Materials",
        "data": {
            "copper alloy": {"id": 701, "nterm": "copper alloy"},
            "bronze": {"id": 701, "nterm": "copper alloy"},
        },
    }
    for key in ("enter", "tab"):
        with launch(binary, thesaurus=graph) as editor:
            editor.call("input", text="cop")
            editor.advance()
            editor.press(key)
            editor.advance()
            assert editor.document() == "copper alloy", editor.document()
            original = visible_indicator(editor, (0, len("copper alloy")))
            editor.press("home")
            editor.press("right")
            editor.press("alt-down")
            assert editor.document() == "bronze", editor.document()
            cycled = visible_indicator(editor, (0, len("bronze")))
            assert original["active"] != cycled["active"]
            editor.press("right")
            editor.press("alt-up")
            assert editor.document() == "copper alloy", editor.document()
            assert visible_indicator(editor, (0, len("copper alloy")))["active"] == original["active"]
    print("PASS loaded role JSON on Enter and Tab: cop -> copper alloy -> bronze -> copper alloy")


def check_kg_record(binary, role_path, canonical, old_synonym, new_synonym):
    original = json.loads(role_path.read_text(encoding="utf-8"))
    record = original["data"][canonical]
    retained = {
        term: entry for term, entry in original["data"].items()
        if entry["id"] != record["id"]
    }

    def open_synonyms(editor):
        editor.press("home")
        editor.press("right")
        editor.press("ctrl-shift-a")
        editor.call("bounds", id="kg-synonyms-panel")

    def replace_synonyms(editor, synonyms):
        editor.press(f"{TEXT_MODIFIER}-a")
        editor.type(synonyms)

    with launch(binary, thesaurus=role_path) as editor:
        editor.type(canonical)
        open_synonyms(editor)
        replace_synonyms(editor, new_synonym)
        assert editor.document() == canonical, "The draft changed the Markdown document"
        editor.click("kg-save-synonyms")
        assert editor.document() == canonical, "Saving changed the Markdown document"
        saved = json.loads(role_path.read_text(encoding="utf-8"))
        assert saved["name"] == original["name"]
        assert old_synonym not in saved["data"]
        assert saved["data"][new_synonym]["id"] == record["id"]
        assert saved["data"][canonical] == record
        assert all(saved["data"][term] == entry for term, entry in retained.items())
        assert {key: value for key, value in saved.items() if key != "data"} == {
            key: value for key, value in original.items() if key != "data"
        }
        editor.press("home")
        editor.press("right")
        editor.press("alt-down")
        assert editor.document() == new_synonym, repr(editor.document())
        visible_indicator(editor, (0, len(new_synonym)))

    with launch(binary, thesaurus=role_path) as editor:
        editor.type(new_synonym)
        visible_indicator(editor, (0, len(new_synonym)))
        open_synonyms(editor)
        replace_synonyms(editor, "uncommitted synonym")
        externally_edited = {**saved, "external_edit": True}
        external_bytes = json.dumps(externally_edited)
        role_path.write_text(external_bytes, encoding="utf-8")
        editor.click("kg-save-synonyms")
        editor.call("bounds", id="kg-synonyms-panel")
        assert role_path.read_text(encoding="utf-8") == external_bytes
        assert editor.document() == new_synonym
        editor.click("kg-cancel-synonyms")
        assert role_path.read_text(encoding="utf-8") == external_bytes
    print("PASS role synonym editing, immediate cycling, reopen, and external-edit protection")


def check_editable_kg(binary, working_dir):
    role_path = working_dir / "editable-kg.json"
    canonical = {
        "id": 701, "nterm": "copper alloy", "display_value": "Copper Alloy",
        "url": "https://example.org/materials/copper-alloy", "pinned": True,
        "custom_metadata": {"retain": True},
    }
    thesaurus = {
        "name": "Editable materials",
        "custom_metadata": ["keep"],
        "data": {
            "copper alloy": canonical,
            "bronze": canonical,
            "aluminium": {"id": 702, "nterm": "aluminium"},
        },
    }
    role_path.write_text(json.dumps(thesaurus), encoding="utf-8")
    check_kg_record(binary, role_path, "copper alloy", "bronze", "bell metal")


def check_authored_alternatives(binary):
    with launch(binary) as editor:
        editor.type("paperclip")
        editor.press(f"{TEXT_MODIFIER}-a")
        editor.press("ctrl-shift-a")
        editor.type("eraser")
        editor.press("enter")
        assert editor.document() == "eraser", repr(editor.document())
        assert visible_indicator(editor, (0, len("eraser")))["active"] == 1
        editor.press(f"{TEXT_MODIFIER}-z")
        assert editor.document() == "paperclip", repr(editor.document())
        assert editor.call("indicators") == []
        editor.press(REDO)
        assert editor.document() == "eraser", repr(editor.document())
        assert visible_indicator(editor, (0, len("eraser")))["active"] == 1
    print("PASS inline authored alternative with highlight, undo and redo")


def check_preview_and_ids(binary):
    with launch(binary) as editor:
        editor.type("Markdown preview paragraph.")
        editor.click("terraphim-ids-btn")
        original = editor.document()
        assert "<!-- terraphim:block-id:" in original, repr(original)
        assert "Markdown preview paragraph." in original
        editor.click("markdown-preview")
        bounds = editor.call("bounds", id="markdown-preview-pane")
        assert bounds[2] > 0 and bounds[3] > 0, bounds
        assert editor.document() == original
    print("PASS block ID insertion and Markdown preview layout")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="Terraphim Editor binary built with --features gdp")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)

    check_slash(binary)
    with tempfile.TemporaryDirectory(prefix="terraphim-gdp-fixtures-") as working_dir:
        directory = Path(working_dir)
        fixture = directory / "blocks.md"
        fixture.write_text(SAVED_DOCUMENT, encoding="utf-8")
        check_blocks(binary, fixture)
        check_editable_kg(binary, directory)
    check_alternatives(binary)
    check_knowledge_graph(binary)
    check_authored_alternatives(binary)
    check_preview_and_ids(binary)


if __name__ == "__main__":
    main()
