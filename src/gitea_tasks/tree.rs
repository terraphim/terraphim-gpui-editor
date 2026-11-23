//! Declaration reader for the Gitea fork's repository-local dependency graph.

use super::{valid_repository_part, GiteaTasks, IssueRef};
use crate::dependency_tree::{add_relation, project, Branch, Relations, TreeGroup};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::{collections::BTreeMap, ffi::OsStr};

#[derive(Debug)]
pub(crate) struct TaskTree {
    pub(crate) groups: Vec<TreeGroup>,
    pub(crate) cached: bool,
}

#[derive(Deserialize)]
struct Graph {
    repo_name: String,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Deserialize)]
struct Node {
    id: u64,
    index: u64,
    title: String,
    is_closed: bool,
}

#[derive(Deserialize)]
struct Edge {
    from: u64,
    to: u64,
    #[serde(rename = "type")]
    kind: String,
    gate: Option<String>,
}

impl GiteaTasks {
    pub(crate) fn tree(&self, issue: &IssueRef) -> Result<TaskTree> {
        ensure!(
            valid_repository_part(&issue.owner)
                && valid_repository_part(&issue.repo)
                && issue.number > 0,
            "Invalid Gitea issue reference"
        );
        let output = self.run([
            OsStr::new("graph"),
            OsStr::new("--owner"),
            OsStr::new(&issue.owner),
            OsStr::new("--repo"),
            OsStr::new(&issue.repo),
        ])?;
        let graph: Graph = serde_json::from_str(&output.stdout)
            .context("Could not read the Gitea dependency graph")?;
        Ok(TaskTree {
            groups: graph.issue_tree(issue)?,
            cached: output.cached_at.is_some(),
        })
    }
}

impl Graph {
    fn issue_tree(self, issue: &IssueRef) -> Result<Vec<TreeGroup>> {
        ensure!(
            self.repo_name.eq_ignore_ascii_case(&issue.repo)
                || self
                    .repo_name
                    .eq_ignore_ascii_case(&format!("{}/{}", issue.owner, issue.repo)),
            "The Gitea dependency graph is for a different repository"
        );
        let mut ids = BTreeMap::new();
        let mut labels = BTreeMap::new();
        for node in self.nodes {
            if node.id == 0 || node.index == 0 {
                continue;
            }
            let key = node.index.to_string();
            let status = if node.is_closed { "Closed" } else { "Open" };
            ensure!(
                ids.insert(node.id, key.clone()).is_none()
                    && labels
                        .insert(key, format!("#{} {} · {status}", node.index, node.title))
                        .is_none(),
                "The Gitea dependency graph contains duplicate issue identities"
            );
        }
        let root = issue.number.to_string();
        ensure!(
            labels.contains_key(&root),
            "This issue is unavailable in the repository dependency graph"
        );
        let mut subtasks = Relations::new();
        let mut prerequisites = Relations::new();
        let mut dependants = Relations::new();
        for edge in self.edges {
            let (Some(from), Some(to)) = (ids.get(&edge.from), ids.get(&edge.to)) else {
                continue;
            };
            // Robot graph stores child or dependant -> parent or prerequisite.
            let relation = match edge.kind.as_str() {
                "parent-child" => {
                    add_relation(&mut subtasks, to, from, "");
                    continue;
                }
                "blocks" => "",
                "conditional-blocks" => "Conditional block",
                "waits-for" => match edge.gate.as_deref() {
                    Some("any-children") => "Waits for any child",
                    Some("all-children") => "Waits for all children",
                    _ => "Waits for",
                },
                _ => continue,
            };
            add_relation(&mut prerequisites, from, to, relation);
            add_relation(&mut dependants, to, from, relation);
        }
        Ok(project(
            &root,
            &labels,
            &[
                Branch {
                    label: "Subtasks",
                    relations: &subtasks,
                },
                Branch {
                    label: "Depends on",
                    relations: &prerequisites,
                },
                Branch {
                    label: "Unblocks",
                    relations: &dependants,
                },
            ],
        ))
    }
}
