Highly experimental 

## Gitea issues in Blocks

Install gitea-robot (`gtr`) and configure its credentials. The editor uses
`GITEA_URL`, or the URL in `~/.config/terraphim/gitea.env`, to recognise links
to your Gitea instance. Set `TERRAPHIM_GITEA_REPO=owner/repository` to prefill
the repository when creating an issue. Your credentials stay in gtr's
configuration.

- Paste a Gitea issue URL into Markdown and open Blocks to see its card. Use
  **Show details** to read the description or **Open in Gitea** to see the full
  issue.
- **Task tree** shows the selected issue's **Subtasks**, **Depends on** and
  **Unblocks** from `gtr graph`. Expand rows to follow a path, or open an issue
  in Gitea. Nested subtasks come from declared `parent-child` edges. A shared
  prerequisite can appear on multiple paths with the same issue link.
- Cards refresh when you open Blocks, open a document in Blocks, or return
  focus to the window. Use **Refresh** to check one issue immediately. A cached
  result shows when it was last fetched.
- Select **Create Gitea issue** on a Markdown block. Review the destination,
  title, and description, then choose **Create issue**. The editor retains
  the source block and inserts a Markdown link. Undo removes the link from the
  document. The issue remains in Gitea.
- If a write cannot be confirmed, check the repository's issue list before
  trying again.

The headless acceptance test in `tests/headless/gitea_blocks_gdp.py` checks the
workflow against a disposable loopback Gitea instance with a private fixture
repository and an open fixture issue. The task tree shows repository-local
coverage. Graph responses can omit edges. Use Gitea for authoritative readiness.

## Edit knowledge graph alternatives

Load a Markdown KG directory as the active role with
`--kg-dir .terraphim/kg/devops`, or add a named role with
`--role DevOps=.terraphim/kg/devops`. Standalone role JSON also works with
`--thesaurus PATH` or `--role NAME=PATH`. For a generated JSON cache, pass
the source Markdown directory.

Put the caret in a matched term and choose **Edit Alternatives** from the
editor's context menu, or press `Ctrl+Shift+A`. The panel shows the concept
and the source file. Enter one synonym per line and choose **Save synonyms**.

For a Markdown-backed role, the editor updates the source record's
`synonyms::` directive and rebuilds the role. Matching, autocomplete, and
synonym cycling update in the same session. Changes to the role stay
separate from the open document and its undo history. If another process
changes the source, reload the editor before trying again. Selections
outside the graph can have document-specific alternatives.

Choose **Concept tree** in the alternatives panel for a matched Markdown
concept. A role record may declare one canonical level-one heading per
`parent::` or `depends_on::` line. Repeat a directive for multiple relations.
For example, a record headed `# Dispatch` can contain `parent:: Task routing`
and `depends_on:: Runner availability`. Open a row to focus the same concept
from either path. Tab and Shift+Tab move between controls. Enter or Space
activates a focused control. Expanding a tree leaves the note and role files
unchanged.

### Use the same Markdown role from NSH and the CLI

Launch the NSH desktop prototype from a shell with this editor binary on
`PATH` and `TERRAPHIM_KG_DIR` set to the absolute path of the role directory.
NSH uses the editor's local request interface to read and save synonyms,
complete terms, cycle synonyms and read concept trees using the same Rust code as GPUI.

To make a current Grep thesaurus from the source role:

```sh
terraphim-gpui-editor --kg-export /absolute/path/to/role > role.json
terraphim-grep "new synonym" --thesaurus role.json --paths ./corpus --search-only --json
```

Configure a Terraphim Agent role's `kg.knowledge_graph_local` with
`input_type: "markdown"` and the same source directory. Then run
`terraphim-agent --config agent.json validate --json "new synonym"`.
