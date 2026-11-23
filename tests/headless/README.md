# Headless editor checks

This opt-in prototype implements a subset of the
[FastGPUI debug protocol](https://github.com/AnswerDotAI/fastgpui) on the
editor's existing GPUI 0.2 test platform. It uses the same editor startup,
document model, key bindings, and controls as the desktop app. The Omarchy
build uses the default Cargo features.

On macOS or Linux, run:

```sh
cargo build --features gdp --bin terraphim-gpui-editor
python3 tests/headless/editor_gdp.py target/debug/terraphim-gpui-editor
```

The Python test starts the process with `GDP_SOCKET` in a private temporary
directory. The driver reads one JSON request per line and responds with the
FastGPUI success or error envelope. Send `{"cmd":"commands"}` to see the
supported operations. The script exercises the `se`, `/se`, and
`/sesearch` shortcuts on Enter and Tab, block buttons, undo and redo, file
saving, and synonym cycling.

The upstream Python client can connect with `GDP(socket_path)` after the
editor starts. To use its launcher, point `XDG_RUNTIME_DIR` to a directory
owned by your user with mode 0700.

The indicator check reads underline and active-dot geometry submitted by the
editor's paint callback. GPUI 0.2 supplies deterministic test font metrics
and discards the final scene, so this checks paint geometry without
producing pixel screenshots. Tray behavior and OMA sandbox integration still
need their native environments.

The loaded-role check starts the editor with a JSON thesaurus through
`--thesaurus`. It types `cop`, accepts `copper alloy` with Enter or Tab,
and verifies that Alt+Down cycles the highlighted term to `bronze`.
Alt+Up restores the original term and the original active dot.


### Task and concept TreeViews

Run against an explicitly configured disposable loopback Gitea fork, using
the `editor-test/blocks-demo` fixture repository. The fixture directory's
`issues.json` maps `root`, `child`, `grandchild`, `left`, `right`, `shared`,
and `downstream` to issue numbers. Create child → root and grandchild → child
as `parent-child`, root → left as `blocks`, root → right as
`conditional-blocks`, left/right → shared as `blocks`, and downstream → root
as `waits-for`. Relation direction here is the Gitea API's stored orientation.
The test reads these real issues and edges.

Include a `kg/` fixture directory with `# Coordination` (alias
`coordination alias`), nested `# Task routing` and `# Dispatch`, and two
declared dependency paths to `# Shared prerequisite`. The test copies the
role before opening it in the editor.

```sh
python3 tests/headless/dependency_tree_gdp.py \
  target/debug/terraphim-gpui-editor /absolute/path/to/fixture
```
