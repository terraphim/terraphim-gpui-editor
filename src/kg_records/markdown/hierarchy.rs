//! Declared hierarchy for a Markdown role.

use super::{lines_with_ranges, normalize, protected_ranges, MarkdownSource};
use crate::dependency_tree::{add_relation, project, Branch, Relations, TreeGroup};
use anyhow::Result;
use markdown::{mdast::Node, ParseOptions};
use std::collections::BTreeMap;

impl MarkdownSource {
    pub(in crate::kg_records) fn concept_tree(&self, id: u64) -> Result<Vec<TreeGroup>> {
        self.concept(id)?;
        let mut names = BTreeMap::new();
        let mut declarations = Vec::new();
        let mut labels = BTreeMap::new();
        for (id, concept) in &self.concepts {
            labels.insert(id.to_string(), concept.term.display().to_owned());
            let source = &self.documents[&concept.path];
            let (name, directives) = hierarchy_in(source);
            if let Some(name) = name {
                names
                    .entry(name)
                    .and_modify(|candidate| *candidate = None)
                    .or_insert_with(|| Some(id.to_string()));
            }
            declarations.push((id.to_string(), directives));
        }

        let mut parents = Relations::new();
        let mut children = Relations::new();
        let mut dependencies = Relations::new();
        let mut dependants = Relations::new();
        for (from, directives) in declarations {
            for (kind, name) in directives {
                let Some(Some(to)) = names.get(&name) else {
                    continue;
                };
                let (forward, reverse) = match kind {
                    Kind::Parent => (&mut parents, &mut children),
                    Kind::Dependency => (&mut dependencies, &mut dependants),
                };
                add_relation(forward, &from, to, "");
                add_relation(reverse, to, &from, "");
            }
        }

        Ok(project(
            &id.to_string(),
            &labels,
            &[
                Branch {
                    label: "Subconcepts",
                    relations: &children,
                },
                Branch {
                    label: "Depends on",
                    relations: &dependencies,
                },
                Branch {
                    label: "Parent concepts",
                    relations: &parents,
                },
                Branch {
                    label: "Dependants",
                    relations: &dependants,
                },
            ],
        ))
    }
}

enum Kind {
    Parent,
    Dependency,
}

fn hierarchy_in(source: &str) -> (Option<String>, Vec<(Kind, String)>) {
    let mut options = ParseOptions::gfm();
    options.constructs.frontmatter = true;
    let Ok(document) = markdown::to_mdast(source, &options) else {
        return (None, Vec::new());
    };
    let name = document.children().and_then(|nodes| {
        nodes
            .iter()
            .find(|node| matches!(node, Node::Heading(heading) if heading.depth == 1))
            .map(|heading| normalize(heading.to_string().trim()))
            .filter(|name| !name.is_empty())
    });
    let mut ranges = protected_ranges(&document);
    if let Some(nodes) = document.children() {
        for node in nodes {
            if let Node::Yaml(frontmatter) = node {
                if let Some(position) = &frontmatter.position {
                    ranges.push(position.start.offset..position.end.offset);
                }
            }
        }
    }
    let directives = lines_with_ranges(source, ranges)
        .filter(|&(_, protected)| !protected)
        .filter_map(|(line, _)| {
            let (key, name) = line.trim().split_once("::")?;
            let kind = match key.to_ascii_lowercase().as_str() {
                "parent" => Kind::Parent,
                "depends_on" => Kind::Dependency,
                _ => return None,
            };
            let name = normalize(name.trim());
            (!name.is_empty()).then_some((kind, name))
        })
        .collect();
    (name, directives)
}
