//! Paint passive alternative indicators over the native text input.
use gpui::{
    canvas, fill, point, px, quad, rgba, size, App, BorderStyle, Bounds, ContentMask, Entity,
    EntityInputHandler, Hsla, IntoElement, Pixels, Point, Styled, Window,
};
use gpui_component::{input::InputState, Size};
use std::ops::Range;
use terraphim_alternatives::{Document, SpanKind};
use terraphim_lsp_core::KgEngine;

#[cfg(feature = "gdp")]
#[derive(Default)]
struct PaintSnapshot(std::cell::RefCell<Vec<serde_json::Value>>);

#[cfg(feature = "gdp")]
impl gpui::Global for PaintSnapshot {}

#[cfg(feature = "gdp")]
pub(crate) fn clear_snapshot(cx: &mut App) {
    cx.default_global::<PaintSnapshot>().0.borrow_mut().clear();
}

#[cfg(feature = "gdp")]
pub(crate) fn paint_snapshot(cx: &App) -> serde_json::Value {
    cx.try_global::<PaintSnapshot>().map_or_else(
        || serde_json::json!([]),
        |snapshot| serde_json::json!(*snapshot.0.borrow()),
    )
}

#[cfg(feature = "gdp")]
fn record_paint(cx: &App, mark: &Indicator, bounds: Bounds<Pixels>, underline_count: usize) {
    cx.global::<PaintSnapshot>()
        .0
        .borrow_mut()
        .push(serde_json::json!({
            "start": mark.range.start,
            "end": mark.range.end,
            "active": mark.active,
            "count": mark.count,
            "underline_count": underline_count,
            "bounds": {
                "x": f32::from(bounds.origin.x),
                "y": f32::from(bounds.origin.y),
                "width": f32::from(bounds.size.width),
                "height": f32::from(bounds.size.height),
            },
        }));
}

struct Indicator {
    range: Range<usize>,
    kind: SpanKind,
    breaks: Vec<usize>,
    count: usize,
    active: usize,
}

impl Indicator {
    fn from_document(document: &Document) -> Vec<Self> {
        document
            .annotations
            .spans
            .iter()
            .filter(|span| !span.is_inert())
            .map(|span| {
                let mut offset = span.anchor.start;
                let mut breaks = Vec::new();
                if span.kind != SpanKind::Paragraph {
                    for character in span.anchor.text.chars() {
                        if character == '\n' {
                            breaks.push(offset);
                        }
                        offset += character.len_utf16();
                    }
                }
                Self {
                    range: span.anchor.start..span.anchor.end,
                    kind: span.kind,
                    breaks,
                    count: span.alts.len(),
                    active: span.active,
                }
            })
            .collect()
    }
}

/// Overlay sibling of the Input, rendered after it so the input layout is current.
pub(crate) fn render(
    input: Entity<InputState>,
    document: &Document,
    graph: Option<&KgEngine>,
) -> impl IntoElement {
    let indicators = collect_indicators(document, graph);
    let body_size = document.body.len();
    canvas(
        |_, _, _| (),
        move |viewport, (), window, cx| {
            #[cfg(feature = "gdp")]
            clear_snapshot(cx);
            if !indicators.is_empty() && input.read(cx).text().len() == body_size {
                paint(&input, &indicators, viewport, window, cx);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn collect_indicators(document: &Document, graph: Option<&KgEngine>) -> Vec<Indicator> {
    let mut indicators = Indicator::from_document(document);
    if let Some(graph) = graph {
        indicators.extend(
            graph
                .synonym_positions(&document.body)
                .into_iter()
                .filter_map(|synonym| {
                    let range = synonym.term.range.start.utf16..synonym.term.range.end.utf16;
                    let covered_by_authored =
                        document.annotations.spans.iter().any(|span| {
                            span.anchor.start < range.end && range.start < span.anchor.end
                        });
                    (!covered_by_authored).then_some(Indicator {
                        range,
                        kind: SpanKind::Word,
                        breaks: Vec::new(),
                        count: synonym.count,
                        active: synonym.index - 1,
                    })
                }),
        );
    }
    indicators
}

fn paint(
    input: &Entity<InputState>,
    indicators: &[Indicator],
    viewport: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let mut content = viewport;
    // InputStyle::input_px/py and the one pixel native input border.
    content.origin += point(
        Size::Medium.input_px() + px(1.),
        Size::Medium.input_py() + px(1.),
    );
    let marks = input.update(cx, |input, cx| {
        indicators
            .iter()
            .filter_map(|mark| {
                let mut locate =
                    |offset| input.bounds_for_range(offset..offset, content, window, cx);
                let first = locate(mark.range.start)?;
                let last = locate(mark.range.end)?;
                if first == last {
                    return None;
                }
                if mark.kind == SpanKind::Paragraph {
                    let height = last.bottom() - first.top();
                    if height <= px(0.) {
                        return None;
                    }
                    return Some((
                        mark,
                        vec![Bounds::new(
                            point(content.left(), first.top()),
                            size(px(1.), height),
                        )],
                    ));
                }
                let mut rows = Vec::new();
                let mut start = first;
                for line_end in &mark.breaks {
                    let end = locate(*line_end)?;
                    push_rows(&mut rows, start, end, viewport);
                    start = locate(*line_end + 1)?;
                }
                push_rows(&mut rows, start, last, viewport);
                Some((mark, rows))
            })
            .collect::<Vec<_>>()
    });

    window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
        let lavender = Hsla::from(rgba(0x8c86_e6ff));
        let muted = Hsla::from(rgba(0x8c86_e666));
        for (mark, rows) in marks {
            let Some(last) = rows.last() else { continue };
            if mark.kind == SpanKind::Paragraph {
                let rule = Bounds::new(point(last.left() - px(4.), last.top()), last.size);
                if rule.intersects(&viewport) {
                    #[cfg(feature = "gdp")]
                    record_paint(cx, mark, rule, 0);
                    window.paint_quad(fill(rule, muted));
                    for index in 0..mark.count {
                        dot(
                            window,
                            point(rule.left() - px(5.), rule.top() + dot_offset(index)),
                            index == mark.active,
                            lavender,
                            muted,
                        );
                    }
                }
                continue;
            }
            #[cfg(feature = "gdp")]
            record_paint(cx, mark, *last, rows.len());
            for row in &rows {
                window.paint_quad(fill(
                    Bounds::new(
                        point(row.left(), row.bottom() - px(4.)),
                        size(row.size.width, px(1.)),
                    ),
                    muted,
                ));
            }
            let group_width = dot_offset(mark.count - 1) + px(3.);
            let dot_start = if mark.kind == SpanKind::Word {
                last.left() + (last.size.width - group_width) / 2.
            } else {
                last.right() - group_width
            };
            for index in 0..mark.count {
                dot(
                    window,
                    point(dot_start + dot_offset(index), last.bottom() - px(2.)),
                    index == mark.active,
                    lavender,
                    muted,
                );
            }
        }
    });
}

fn push_rows(
    rows: &mut Vec<Bounds<Pixels>>,
    start: Bounds<Pixels>,
    end: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
) {
    let height = start.size.height;
    if end.top() < start.top() || height <= px(0.) {
        return;
    }

    let mut left = start.left();
    let mut top = start.top();
    while top <= end.top() && top < viewport.bottom() {
        let right = if top == end.top() {
            end.right()
        } else {
            viewport.right() - px(13.)
        };
        if right > left {
            let row = Bounds::new(point(left, top), size(right - left, height));
            if row.intersects(&viewport) {
                rows.push(row);
            }
        }
        left = viewport.left() + px(13.);
        top += height;
    }
}

#[allow(clippy::cast_precision_loss)]
fn dot_offset(index: usize) -> Pixels {
    px(6. * index as f32)
}

fn dot(window: &mut Window, position: Point<Pixels>, active: bool, lavender: Hsla, muted: Hsla) {
    let bounds = Bounds::new(position, size(px(3.), px(3.)));
    let background = if active {
        lavender
    } else {
        gpui::transparent_black()
    };
    window.paint_quad(quad(
        bounds,
        px(1.5),
        background,
        px(0.75),
        muted,
        BorderStyle::default(),
    ));
}

#[cfg(test)]
mod tests {
    use super::collect_indicators;
    use terraphim_alternatives::{Document, Source, SpanKind};
    use terraphim_lsp_core::KgEngine;

    #[test]
    fn indicator_tracks_authored_active_choice_and_omits_inert_spans() {
        let mut document = Document::new("🦀 Clip.");
        let id = document.add_span(SpanKind::Word, 3, 7).unwrap();
        assert_eq!(collect_indicators(&document, None).len(), 0);

        document
            .add_alternative(&id, "Pin", Source::Human, None)
            .unwrap();
        document.set_active(&id, 1).unwrap();
        let marks = collect_indicators(&document, None);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].range, 3..6);
        assert_eq!((marks[0].count, marks[0].active), (2, 1));

        document.clear_alternatives(&id).unwrap();
        assert_eq!(collect_indicators(&document, None).len(), 0);
    }

    #[test]
    fn kg_indicators_respect_the_current_role_and_authored_spans() {
        let kg = KgEngine::from_json(
            r#"{"name":"Tools","data":{
            "clip":{"id":1,"nterm":"clip"},"pin":{"id":1,"nterm":"clip"}
        }}"#,
        )
        .unwrap();
        let mut document = Document::new("🦀 Pin.");
        let marks = collect_indicators(&document, Some(&kg));
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].range, 3..6);
        assert_eq!((marks[0].count, marks[0].active), (2, 1));

        let id = document.add_span(SpanKind::Word, 3, 6).unwrap();
        document
            .add_alternative(&id, "Needle", Source::Human, None)
            .unwrap();
        let marks = collect_indicators(&document, Some(&kg));
        assert_eq!(marks.len(), 1);
        assert_eq!((marks[0].count, marks[0].active), (2, 0));
    }
}
