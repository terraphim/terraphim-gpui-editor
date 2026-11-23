"""Exercise expand, shared identity and concept selection in real GPUI views."""

import argparse
import json
from pathlib import Path
import shutil
import tempfile

from editor_gdp import launch
from gitea_blocks_gdp import eventually, local_origin
from markdown_kg_gdp import enter_document, place_caret_in_term


def branch_contains_shared(editor, scope, group, branch):
    selector = f"{scope}-{group}-{branch}"
    editor.click(f"{selector}-toggle")
    return f"{selector}-0-open"


def check_gitea(binary, issue):
    origin = local_origin()
    work = json.loads(issue.read_text(encoding="utf-8"))
    with launch(binary) as editor:
        issue_url = f"{origin}/editor-test/blocks-demo/issues/{work['root']}"
        enter_document(editor, issue_url)
        editor.press("ctrl-shift-b")
        editor.click("gitea-task-tree-0")
        cards = eventually(
            editor,
            lambda: editor.call("gitea_cards"),
            lambda cards: bool(cards and cards[0].get("tree_groups") is not None),
            "Gitea dependency tree did not load",
        )
        groups = cards[0]["tree_groups"]
        assert [group["label"] for group in groups] == [
            "Subtasks", "Depends on", "Unblocks"
        ]
        child = groups[0]["nodes"][0]
        assert child["id"] == str(work["child"])
        assert child["children"][0]["id"] == str(work["grandchild"])
        left, right = groups[1]["nodes"]
        assert right["label"].endswith("Conditional block")
        shared = [left["children"][0], right["children"][0]]
        assert [node["id"] for node in shared] == [str(work["shared"])] * 2
        assert all(node["shared"] for node in shared)
        assert groups[2]["nodes"][0]["id"] == str(work["downstream"])
        assert "Waits for" in groups[2]["nodes"][0]["label"]

        editor.click("task-tree-0-0-0-toggle")
        editor.call("bounds", id="task-tree-0-0-0-0-open")
        editor.click("task-tree-0-0-0-toggle")
        for index in range(2):
            selector = branch_contains_shared(editor, "task-tree-0", 1, index)
            editor.call("bounds", id=selector)
            editor.click(f"task-tree-0-1-{index}-toggle")
        assert editor.document() == issue_url


def check_concepts(binary, fixture, workspace):
    role = workspace / "role"
    shutil.copytree(fixture, role)
    original = {path: path.read_bytes() for path in role.glob("*.md")}
    with launch(binary, extra_args=["--kg-dir", str(role)]) as editor:
        enter_document(editor, "coordination alias")
        place_caret_in_term(editor)
        editor.press("ctrl-shift-a")
        editor.press("tab")
        editor.press("tab")
        editor.press("enter")
        tree = editor.call("concept_tree")
        original_id = tree["concept_id"]
        assert tree["label"] == "Coordination"
        assert [group["label"] for group in tree["groups"]] == [
            "Subconcepts", "Depends on"
        ]
        assert tree["groups"][0]["nodes"][0]["children"][0]["label"] == "Dispatch"
        shared = [node["children"][0] for node in tree["groups"][1]["nodes"]]
        assert len(shared) == 2
        assert all(node["label"] == "Shared prerequisite" and node["shared"] for node in shared)
        assert shared[0]["id"] == shared[1]["id"]

        editor.press("tab")
        editor.press("tab")
        editor.press("space")
        editor.call("bounds", id="concept-tree-0-0-0-open")
        editor.click("concept-tree-0-0-toggle")
        for index in range(2):
            selector = branch_contains_shared(editor, "concept-tree", 1, index)
            editor.click(selector)
            focused = editor.call("concept_tree")
            assert focused["concept_id"] == shared[0]["id"]
            editor.click("kg-tree-back")
            assert editor.call("concept_tree")["concept_id"] == original_id
        assert editor.document() == "coordination alias"
        editor.click("kg-cancel-synonyms")
        place_caret_in_term(editor)
        editor.press("alt-down")
        assert editor.document().casefold() == "coordination"
    assert all(path.read_bytes() == content for path, content in original.items())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("fixture", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="dependency-tree-") as workspace:
        check_gitea(args.binary.resolve(), args.fixture / "issues.json")
        check_concepts(args.binary.resolve(), args.fixture / "kg", Path(workspace))
    print("GPUI task and concept TreeViews passed against real Gitea and Markdown fixtures")


if __name__ == "__main__":
    main()
