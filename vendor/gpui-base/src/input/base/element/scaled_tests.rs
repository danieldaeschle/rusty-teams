#![cfg(target_os = "linux")]

use std::ops::Range;

use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler as _, HighlightStyle, IntoElement,
    ParentElement as _, Pixels, Render, Styled as _, TestAppContext, TextRun, VisualTestContext,
    Window, div, point, px, size,
};

use super::TextElement;
use crate::input::{
    InlineToken, InlineTokenPresentation, InputContent, MoveDown, MoveUp, TextDecoration,
    TextareaMode, TextareaState,
};

const LARGE: f32 = 24. / 14.;
const SMALL: f32 = 9. / 14.;

struct Harness(Entity<TextareaState>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

fn scaled(range: Range<usize>, scale: f32) -> TextDecoration {
    TextDecoration::new(range, HighlightStyle::default()).with_font_scale(scale)
}

fn raised(range: Range<usize>, scale: f32, raise: f32) -> TextDecoration {
    scaled(range, scale).with_baseline_shift(raise)
}

fn near(left: Pixels, right: Pixels) -> bool {
    (left - right).abs() < px(0.05)
}

fn draw(visual: &mut VisualTestContext) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

fn view(
    text: &str,
    width: f32,
    height: f32,
    decorations: Vec<TextDecoration>,
) -> (Entity<TextareaState>, VisualTestContext) {
    build(text, width, height, decorations, |state| state.rows(8))
}

fn build(
    text: &str,
    width: f32,
    height: f32,
    decorations: Vec<TextDecoration>,
    configure: impl FnOnce(TextareaState) -> TextareaState,
) -> (Entity<TextareaState>, VisualTestContext) {
    let platform = gpui_platform::current_platform(true);
    let mut cx = TestAppContext::build_with_text_system(
        gpui::TestDispatcher::new(0),
        None,
        platform.text_system(),
    );
    cx.update(crate::init);
    let mut textarea = None;
    let window = cx.open_window(size(px(width), px(height)), |window, cx| {
        let state =
            cx.new(|cx| configure(TextareaState::new(window, cx)).default_value(text.to_owned()));
        textarea = Some(state.clone());
        Harness(state)
    });
    let textarea = textarea.unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    visual.update(|window, cx| {
        textarea.update(cx, |state, cx| {
            state.create_decorations_collection(decorations, cx);
        });
        window.draw(cx).clear(cx);
    });
    draw(&mut visual);
    (textarea, visual)
}

fn base_size(visual: &mut VisualTestContext) -> Pixels {
    visual.update(|window, _| window.text_style().font_size.to_pixels(window.rem_size()))
}

fn shaped_width(visual: &mut VisualTestContext, text: &str, font_size: Pixels) -> Pixels {
    visual.update(|window, _| {
        window
            .text_system()
            .shape_line(
                text.to_owned().into(),
                font_size,
                &[TextRun {
                    len: text.len(),
                    font: window.text_style().font(),
                    color: gpui::black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
            .width
    })
}

fn line_height(textarea: &Entity<TextareaState>, visual: &VisualTestContext) -> Pixels {
    textarea.read_with(visual, |state, _| state.line_height().unwrap())
}

#[test]
fn scaled_ranges_become_baseline_aligned_fragments_of_their_own_size() {
    let (textarea, mut visual) = view("abc DEF ghi", 400., 200., vec![scaled(4..7, LARGE)]);
    let font_size = base_size(&mut visual);
    let large_width = shaped_width(&mut visual, "DEF", font_size * LARGE);
    let large_first = shaped_width(&mut visual, "D", font_size * LARGE);
    let plain_width = shaped_width(&mut visual, "abc ", font_size);
    let lh = line_height(&textarea, &visual);
    textarea.read_with(&visual, |state, _| {
        let layout = state.last_layout.as_ref().unwrap();
        let row = &layout.lines[0].wrapped_lines[0];
        let fragments = row.fragments().expect("scaled row is fragmented");
        assert_eq!(fragments.len(), 3);
        assert_eq!(fragments[0].range, 0..4);
        assert_eq!(fragments[1].range, 4..7);
        assert!(near(fragments[1].x, plain_width));
        assert!(near(fragments[1].width, large_width));
        assert!(near(row.x_for_index(4), fragments[1].x));
        assert!(near(row.x_for_index(7), fragments[2].x));
        assert!(near(row.x_for_index(5), fragments[1].x + large_first));
        let height = row.height.expect("Large grows the row");
        assert!(height > lh * 1.5, "{height:?} vs {lh:?}");
        let baselines: Vec<_> = fragments
            .iter()
            .map(|fragment| {
                let (y, paint_height) = fragment.placement.unwrap();
                let shaped = fragment.text.as_ref().unwrap();
                assert!(near(paint_height, shaped.ascent + shaped.descent));
                y + shaped.ascent
            })
            .collect();
        assert!(near(baselines[0], baselines[1]) && near(baselines[1], baselines[2]));
        assert_eq!(state.display_map.row_height(0, layout.line_height), height);
    });
}

#[test]
fn small_text_never_shrinks_the_row() {
    let (textarea, mut visual) = view("abc DEF ghi", 400., 200., vec![scaled(4..7, SMALL)]);
    let font_size = base_size(&mut visual);
    let small_width = shaped_width(&mut visual, "DEF", font_size * SMALL);
    let lh = line_height(&textarea, &visual);
    textarea.read_with(&visual, |state, _| {
        let layout = state.last_layout.as_ref().unwrap();
        let row = &layout.lines[0].wrapped_lines[0];
        assert!(near(row.fragments().unwrap()[1].width, small_width));
        assert_eq!(row.height, None);
        assert_eq!(state.display_map.row_height(0, lh), lh);
        assert_eq!(state.display_map.content_height(lh), lh);
    });
}

#[test]
fn superscript_and_subscript_shift_the_baseline_and_grow_the_row() {
    let (textarea, mut visual) = view(
        "H2O x2",
        400.,
        200.,
        vec![raised(1..2, 0.75, -0.2), raised(5..6, 0.75, 0.35)],
    );
    let font_size = base_size(&mut visual);
    let lh = line_height(&textarea, &visual);
    textarea.read_with(&visual, |state, _| {
        let layout = state.last_layout.as_ref().unwrap();
        let row = &layout.lines[0].wrapped_lines[0];
        let fragments = row.fragments().unwrap();
        let baseline = |index: usize| {
            let fragment = &fragments[index];
            fragment.placement.unwrap().0 + fragment.text.as_ref().unwrap().ascent
        };
        assert_eq!(fragments.len(), 4);
        assert!(near(baseline(1) - baseline(0), font_size * 0.2));
        assert!(near(baseline(0) - baseline(3), font_size * 0.35));
        assert!(row.height.unwrap() > lh);
        assert_eq!(state.display_map.row_height(0, lh), row.height.unwrap());
    });
}

#[test]
fn plain_lines_keep_the_shaped_line_path_and_row_heights_agree_with_the_wrapper() {
    let text = "plain\nabc DEF\n\nplain";
    let (textarea, visual) = view(text, 400., 300., vec![scaled(10..13, LARGE)]);
    let lh = line_height(&textarea, &visual);
    textarea.read_with(&visual, |state, _| {
        let layout = state.last_layout.as_ref().unwrap();
        for (index, plain) in [(0, true), (1, false), (2, true), (3, true)] {
            let row = &layout.lines[index].wrapped_lines[0];
            assert_eq!(row.fragments().is_none(), plain, "line {index}");
            assert_eq!(row.height.is_none(), plain, "line {index}");
        }
        let large = layout.lines[1].wrapped_lines[0].height.unwrap();
        assert_eq!(state.display_map.row_height(1, lh), large);
        assert_eq!(state.display_map.row_top(2, lh), lh + large);
        assert_eq!(state.display_map.content_height(lh), lh * 3. + large);
        assert_eq!(layout.lines[1].size(lh).height, large);
    });
}

#[test]
fn removing_the_scale_restores_plain_rows() {
    let (textarea, mut visual) = view("abc DEF", 400., 200., vec![]);
    let lh = line_height(&textarea, &visual);
    let collection = visual.update(|_, cx| {
        textarea.update(cx, |state, cx| {
            state.create_decorations_collection(vec![scaled(4..7, LARGE)], cx)
        })
    });
    draw(&mut visual);
    textarea.read_with(&visual, |state, _| {
        assert!(state.display_map.row_height(0, lh) > lh);
    });
    visual.update(|_, cx| collection.clear(cx));
    draw(&mut visual);
    textarea.read_with(&visual, |state, _| {
        assert_eq!(state.display_map.row_height(0, lh), lh);
        let layout = state.last_layout.as_ref().unwrap();
        assert!(layout.lines[0].wrapped_lines[0].fragments().is_none());
    });
}

#[test]
fn mixed_sizes_wrap_by_their_real_widths() {
    let text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh";
    let (plain, plain_visual) = view(text, 200., 300., vec![]);
    let (mixed, mixed_visual) = view(text, 200., 300., vec![scaled(10..24, LARGE)]);
    let plain_rows = plain.read_with(&plain_visual, |state, _| {
        state.last_layout.as_ref().unwrap().lines[0]
            .wrapped_lines
            .len()
    });
    mixed.read_with(&mixed_visual, |state, _| {
        let layout = state.last_layout.as_ref().unwrap();
        let line = &layout.lines[0];
        assert!(line.wrapped_lines.len() > plain_rows);
        for row in &line.wrapped_lines {
            assert!(row.width <= layout.wrap_width.unwrap(), "{:?}", row.width);
        }
        let wrapper_rows = state.display_map.line(0).unwrap().wrapped_lines.clone();
        assert_eq!(wrapper_rows.len(), line.wrapped_lines.len());
        for (index, range) in wrapper_rows.iter().enumerate() {
            assert_eq!(line.wrapped_lines[index].len, range.len());
            assert_eq!(
                state.display_map.row_height(index, layout.line_height),
                line.row_height(index, layout.line_height)
            );
        }
        assert!(line.size(layout.line_height).height > layout.line_height * plain_rows as f32);
    });
}

#[test]
fn offsets_map_to_pixels_and_back_across_a_size_change() {
    let text = "ab cd EF gh";
    let (textarea, visual) = view(text, 600., 200., vec![scaled(6..8, LARGE)]);
    textarea.read_with(&visual, |state, _| {
        let layout = state.last_layout.as_ref().unwrap();
        let line = &layout.lines[0];
        let mut previous = px(-1.);
        for offset in 0..=text.len() {
            let position = line.position_for_index(offset, layout, false).unwrap();
            assert!(position.x > previous, "offset {offset}");
            previous = position.x;
            let (hit, _) = line.closest_index_for_position(position, layout).unwrap();
            assert_eq!(hit, offset);
            assert_eq!(line.index_for_position(position, layout), Some(offset));
        }
        let before = line.position_for_index(6, layout, false).unwrap();
        let after = line.position_for_index(8, layout, false).unwrap();
        let (middle, _) = line
            .closest_index_for_position(point((before.x + after.x) / 2. + px(1.), before.y), layout)
            .unwrap();
        assert_eq!(middle, 8);
    });
}

#[test]
fn the_caret_on_a_large_line_spans_its_row() {
    let (textarea, mut visual) = view(
        "one\ntwo BIG\nthree",
        400.,
        300.,
        vec![scaled(8..11, LARGE)],
    );
    let lh = line_height(&textarea, &visual);
    let mut carets = Vec::new();
    for offset in [8, 9] {
        visual.update(|_, cx| {
            textarea.update(cx, |state, cx| state.set_selected_range(offset..offset, cx))
        });
        draw(&mut visual);
        carets.push(textarea.read_with(&visual, |state, _| state.cursor_layout().unwrap().0));
    }
    textarea.read_with(&visual, |state, _| {
        let layout = state.last_layout.as_ref().unwrap();
        let row_height = layout.lines[1].wrapped_lines[0].height.unwrap();
        assert!(near(carets[0].size.height, row_height), "{:?}", carets[0]);
        assert!(carets[0].size.height > lh * 1.4);
        let row_top = layout.visible_top + layout.lines[0].size(lh).height;
        let origin = state.text_bounds().unwrap().origin;
        assert!(near(carets[0].origin.y - origin.y, row_top));
        assert!(near(carets[0].origin.y, carets[1].origin.y));
        let line = &layout.lines[1];
        let step = line.position_for_index(5, layout, false).unwrap().x
            - line.position_for_index(4, layout, false).unwrap().x;
        assert!(near(carets[1].origin.x - carets[0].origin.x, step));
    });
}

#[test]
fn vertical_movement_keeps_the_pixel_column_between_a_large_and_a_normal_line() {
    let text = "iiiiiiii\nBBBBBBBB\niiiiiiii";
    let (textarea, mut visual) = view(text, 400., 300., vec![scaled(9..17, LARGE)]);
    visual.update(|_, cx| textarea.update(cx, |state, cx| state.set_selected_range(4..4, cx)));
    draw(&mut visual);
    let expected = |direction_row: usize, source_row: usize, source_offset: usize| {
        textarea.read_with(&visual, |state, _| {
            let layout = state.last_layout.as_ref().unwrap();
            let x = layout.lines[source_row]
                .position_for_index(source_offset - 9 * source_row, layout, false)
                .unwrap()
                .x;
            let (index, _) = layout.lines[direction_row]
                .closest_index_for_position(point(x, px(0.)), layout)
                .unwrap();
            9 * direction_row + index
        })
    };
    let down_target = expected(1, 0, 4);
    visual.update(|window, cx| textarea.update(cx, |state, cx| state.down(&MoveDown, window, cx)));
    let landed = textarea.read_with(&visual, |state, _| state.cursor());
    assert_eq!(landed, down_target);
    assert!((9..=17).contains(&landed));
    draw(&mut visual);
    visual.update(|window, cx| textarea.update(cx, |state, cx| state.down(&MoveDown, window, cx)));
    let landed_below = textarea.read_with(&visual, |state, _| state.cursor());
    assert!(landed_below >= 18);
    visual.update(|window, cx| textarea.update(cx, |state, cx| state.up(&MoveUp, window, cx)));
    visual.update(|window, cx| textarea.update(cx, |state, cx| state.up(&MoveUp, window, cx)));
    let landed_top = textarea.read_with(&visual, |state, _| state.cursor());
    assert!(landed_top < 9);
}

#[test]
fn selection_boxes_follow_each_rows_height_across_sizes() {
    let text = "ab CD ef\ngh ij";
    let (textarea, visual) = view(text, 400., 300., vec![scaled(3..5, LARGE)]);
    textarea.read_with(&visual, |state, _| {
        let layout = state.last_layout.as_ref().unwrap();
        let corners = TextElement::<TextareaMode>::layout_range_corners(&(1..12), layout).unwrap();
        assert_eq!(corners.len(), 2);
        let first_height = layout.lines[0].wrapped_lines[0].height.unwrap();
        assert!(near(
            corners[0].bottom_left.y - corners[0].top_left.y,
            first_height
        ));
        assert!(near(
            corners[1].bottom_left.y - corners[1].top_left.y,
            layout.line_height
        ));
        assert!(near(corners[1].top_left.y, first_height));
        let row = &layout.lines[0].wrapped_lines[0];
        assert!(near(corners[0].top_left.x, row.x_for_index(1)));
        assert!(near(
            corners[0].top_right.x,
            row.x_for_index(8) + layout.space_width
        ));

        let inside = TextElement::<TextareaMode>::layout_range_corners(&(1..7), layout).unwrap();
        assert_eq!(inside.len(), 1);
        assert!(near(inside[0].top_right.x, row.x_for_index(7)));
        assert!(near(
            inside[0].bottom_right.y - inside[0].top_right.y,
            first_height
        ));
    });
}

#[test]
fn ime_bounds_and_hit_testing_use_the_fragment_geometry() {
    let (textarea, mut visual) = view("ab CD ef", 400., 300., vec![scaled(3..5, LARGE)]);
    let bounds = textarea.read_with(&visual, |state, _| state.text_bounds().unwrap());
    let (row_height, x_start, x_end, line_number_width) =
        textarea.read_with(&visual, |state, _| {
            let layout = state.last_layout.as_ref().unwrap();
            let row = &layout.lines[0].wrapped_lines[0];
            (
                row.height.unwrap(),
                row.x_for_index(3),
                row.x_for_index(5),
                layout.line_number_width,
            )
        });
    let range = visual.update(|window, cx| {
        textarea.update(cx, |state, cx| {
            state.bounds_for_range(3..5, bounds, window, cx).unwrap()
        })
    });
    assert!(near(
        range.origin.x - bounds.origin.x - line_number_width,
        x_start
    ));
    assert!(near(range.size.height, row_height));
    assert!(near(range.right() - range.left(), x_end - x_start));
    let probe = bounds.origin
        + point(
            line_number_width + x_start + (x_end - x_start) * 0.9,
            row_height / 2.,
        );
    let index = visual.update(|window, cx| {
        textarea.update(cx, |state, cx| {
            state.character_index_for_point(probe, window, cx)
        })
    });
    assert_eq!(index, Some(5));
}

#[test]
fn inline_tokens_in_a_scaled_row_share_the_baseline() {
    let (textarea, mut visual) = view("", 400., 300., vec![]);
    visual.update(|window, cx| {
        textarea.update(cx, |state, cx| {
            state.set_token_presentation(
                InlineTokenPresentation::default().token(|_, _, _| div().w(px(30.)).h(px(18.))),
            );
            let content = InputContent::new("ab[x]cd")
                .with_token(2..5, InlineToken::new("x", "[x]"))
                .unwrap();
            state.set_value(content, window, cx);
            state.create_decorations_collection(vec![scaled(0..7, LARGE)], cx);
        });
    });
    draw(&mut visual);
    let lh = line_height(&textarea, &visual);
    textarea.read_with(&visual, |state, _| {
        let layout = state.last_layout.as_ref().unwrap();
        let row = &layout.lines[0].wrapped_lines[0];
        let fragments = row.fragments().unwrap();
        assert_eq!(fragments.len(), 3);
        assert!(fragments[1].text.is_none());
        assert!(near(fragments[1].width, px(30.)));
        assert!(near(fragments[2].x, fragments[1].x + px(30.)));
        assert!(row.height.unwrap() > lh);
        assert!(row.inline_offset > px(0.));
        assert!(near(row.x_for_index(5), fragments[2].x));
        assert_eq!(state.display_map.row_height(0, lh), row.height.unwrap());
        let (token_bounds, _) = state
            .token_bounds
            .iter()
            .next()
            .map(|(_, bounds)| (*bounds, ()))
            .unwrap();
        let top = state.text_bounds().unwrap().origin.y + layout.visible_top;
        assert!(near(token_bounds.origin.y - top, row.inline_offset));
    });
}

#[test]
fn the_caret_stays_in_view_when_large_lines_fill_the_viewport() {
    let text = "WWW\n".repeat(10) + "end";
    let (textarea, mut visual) = view(&text, 300., 120., vec![scaled(0..text.len(), LARGE)]);
    visual.update(|_, cx| {
        textarea.update(cx, |state, cx| {
            state.set_selected_range(text.len()..text.len(), cx)
        })
    });
    draw(&mut visual);
    textarea.read_with(&visual, |state, _| {
        let (caret, _) = state.cursor_layout().unwrap();
        let bounds = state.input_bounds();
        let scroll = state.scroll_handle.offset().y;
        assert!(
            caret.bottom() + scroll <= bounds.bottom() + px(1.),
            "{caret:?} {bounds:?} {scroll:?}"
        );
        assert!(
            caret.top() + scroll >= bounds.top() - px(1.),
            "{caret:?} {bounds:?} {scroll:?}"
        );
    });
}

#[test]
fn an_empty_composer_with_a_scale_decoration_still_shows_its_placeholder() {
    let (textarea, mut visual) = view("", 300., 120., vec![scaled(0..0, LARGE)]);
    let lh = line_height(&textarea, &visual);
    draw(&mut visual);
    textarea.read_with(&visual, |state, _| {
        assert_eq!(state.display_map.content_height(lh), lh);
    });
}

#[test]
fn auto_grow_follows_scaled_rows_by_their_exact_extra() {
    for (decoration, taller_than_a_row) in [
        (raised(1..2, 0.75, -0.2), false),
        (scaled(1..2, LARGE), true),
    ] {
        let (textarea, visual) = build("a2b", 300., 400., vec![decoration], |state| {
            state.auto_grow(1, 5)
        });
        let lh = line_height(&textarea, &visual);
        textarea.read_with(&visual, |state, _| {
            let layout = state.last_layout.as_ref().unwrap();
            let row_height = layout.lines[0].wrapped_lines[0].height.unwrap();
            assert_eq!(state.mode.rows(), 1);
            assert!(near(state.mode.grow_extra(), row_height - lh));
            assert_eq!(row_height > lh * 1.5, taller_than_a_row);
            assert!(row_height < lh * 2.);
        });
    }
}
