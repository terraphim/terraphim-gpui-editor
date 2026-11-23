//! Bounded tree projections over declared task and concept relations.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const MAX_DEPTH: usize = 8;
const MAX_NODES: usize = 128;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TreeGroup {
    pub(crate) label: &'static str,
    pub(crate) nodes: Vec<TreeNode>,
    pub(crate) truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TreeNode {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) shared: bool,
    pub(crate) cycle: bool,
    pub(crate) children: Vec<TreeNode>,
}

pub(crate) type Relations = BTreeMap<String, BTreeSet<(String, &'static str)>>;

pub(crate) struct Branch<'a> {
    pub(crate) label: &'static str,
    pub(crate) relations: &'a Relations,
}

pub(crate) fn add_relation(
    relations: &mut Relations,
    from: impl Into<String>,
    to: impl Into<String>,
    label: &'static str,
) {
    relations
        .entry(from.into())
        .or_default()
        .insert((to.into(), label));
}

pub(crate) fn project(
    root: &str,
    labels: &BTreeMap<String, String>,
    branches: &[Branch<'_>],
) -> Vec<TreeGroup> {
    let mut remaining = MAX_NODES;
    let mut occurrences = BTreeMap::new();
    let mut groups = Vec::new();
    let active: Vec<_> = branches
        .iter()
        .filter(|branch| branch.relations.contains_key(root))
        .collect();

    for (index, branch) in active.iter().enumerate() {
        let budget = remaining / (active.len() - index);
        let mut subtree = Subtree {
            labels,
            relations: branch.relations,
            remaining: budget,
            truncated: false,
            occurrences: &mut occurrences,
        };
        let nodes = subtree.children(root, &mut vec![root.to_owned()]);
        remaining -= budget - subtree.remaining;
        groups.push(TreeGroup {
            label: branch.label,
            nodes,
            truncated: subtree.truncated,
        });
    }
    for group in &mut groups {
        mark_shared(&mut group.nodes, &occurrences);
    }
    groups
}

struct Subtree<'a> {
    labels: &'a BTreeMap<String, String>,
    relations: &'a Relations,
    remaining: usize,
    truncated: bool,
    occurrences: &'a mut BTreeMap<String, usize>,
}

impl Subtree<'_> {
    fn children(&mut self, parent: &str, path: &mut Vec<String>) -> Vec<TreeNode> {
        let Some(edges) = self.relations.get(parent) else {
            return Vec::new();
        };
        if path.len() > MAX_DEPTH {
            self.truncated = true;
            return Vec::new();
        }
        let mut nodes = Vec::new();
        for (id, relation) in edges {
            if self.remaining == 0 {
                self.truncated = true;
                break;
            }
            let Some(label) = self.labels.get(id) else {
                continue;
            };
            self.remaining -= 1;
            let cycle = path.contains(id);
            *self.occurrences.entry(id.clone()).or_default() += 1;
            let children = if cycle {
                Vec::new()
            } else {
                path.push(id.clone());
                let children = self.children(id, path);
                path.pop();
                children
            };
            nodes.push(TreeNode {
                id: id.clone(),
                label: if relation.is_empty() {
                    label.clone()
                } else {
                    format!("{label} · {relation}")
                },
                shared: false,
                cycle,
                children,
            });
        }
        nodes
    }
}

fn mark_shared(nodes: &mut [TreeNode], occurrences: &BTreeMap<String, usize>) {
    for node in nodes {
        node.shared = occurrences.get(&node.id).is_some_and(|count| *count > 1);
        mark_shared(&mut node.children, occurrences);
    }
}
