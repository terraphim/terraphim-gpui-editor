use markdown::{mdast::Node, ParseOptions};
use std::ops::Range;

#[derive(Clone)]
pub(crate) struct Block {
    pub(crate) range: Range<usize>,
    pub(crate) kind: &'static str,
    pub(crate) move_start: usize,
}

pub(crate) fn parse(body: &str) -> Vec<Block> {
    let Ok(Node::Root(root)) = markdown::to_mdast(body, &ParseOptions::gfm()) else {
        return Vec::new();
    };
    let mut id_comment = None;
    root.children
        .iter()
        .filter_map(|node| {
            let pos = node.position()?;
            let range = pos.start.offset..pos.end.offset;
            let text = body.get(range.clone())?;
            let kind = match node {
                Node::Heading(_) => "Heading",
                Node::List(_) => "List",
                Node::Blockquote(_) => "Quote",
                Node::Code(_) => "Code",
                Node::Table(_) => "Table",
                Node::ThematicBreak(_) => "Rule",
                Node::Html(_) if text.trim_start().starts_with("<!-- terraphim:block-id:") => {
                    id_comment = Some(range.start);
                    return None;
                }
                Node::Html(_) => "HTML",
                _ => "Paragraph",
            };
            let move_start = id_comment.take().unwrap_or(range.start);
            Some(Block {
                range,
                kind,
                move_start,
            })
        })
        .collect()
}

pub(crate) fn swap_adjacent(
    document: &mut terraphim_alternatives::Document,
    first: &Block,
    second: &Block,
) -> anyhow::Result<()> {
    use terraphim_alternatives::byte_to_utf16;
    anyhow::ensure!(first.range.end <= second.move_start, "Blocks overlap");
    let offset =
        |byte| byte_to_utf16(&document.body, byte).ok_or_else(|| anyhow::anyhow!("Block is stale"));
    let start = offset(first.move_start)?;
    let end = offset(first.range.end)?;
    let next = offset(second.move_start)?;
    let finish = offset(second.range.end)?;
    let next_width = finish - next;
    let mut candidate = document.clone();
    candidate.move_range(next, finish, start)?;
    if end != next {
        candidate.move_range(end + next_width, finish, start + next_width)?;
    }
    *document = candidate;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse, swap_adjacent};

    #[test]
    fn cards_track_source_bytes_and_exclude_id_comments() {
        let source =
            "<!-- terraphim:block-id:foo -->\r\n# Title\r\n\r\n🦀 body\r\n\r\n- one\r\n- two\r\n";
        let cards = parse(source);
        let text: Vec<_> = cards
            .iter()
            .map(|card| (card.kind, &source[card.range.clone()]))
            .collect();
        assert_eq!(
            text,
            [
                ("Heading", "# Title"),
                ("Paragraph", "🦀 body"),
                ("List", "- one\r\n- two")
            ]
        );
    }

    #[test]
    fn move_carries_alternatives_and_keeps_separator_and_block_id() {
        use terraphim_alternatives::{Document, Source, SpanKind};
        let mut document = Document::new("<!-- terraphim:block-id:first -->\nClip.\n\n🦀 Last.");
        let start = document.body.find("Clip").unwrap();
        let id = document.add_span(SpanKind::Word, start, start + 4).unwrap();
        document
            .add_alternative(&id, "Pin", Source::Human, None)
            .unwrap();
        let cards = parse(&document.body);
        swap_adjacent(&mut document, &cards[0], &cards[1]).unwrap();
        assert_eq!(
            document.body,
            "🦀 Last.\n\n<!-- terraphim:block-id:first -->\nClip."
        );
        let span = document.span(&id).unwrap();
        assert_eq!(span.anchor.text, "Clip");
        assert_eq!(
            span.anchor.start,
            document.body[..document.body.find("Clip").unwrap()]
                .encode_utf16()
                .count()
        );
    }
}
