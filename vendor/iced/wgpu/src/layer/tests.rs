use super::*;
use crate::core::{Font, Pixels, Size, alignment};
use crate::graphics::Layer as _;

fn text(content: &str, clip_bounds: Rectangle) -> Text {
    Text::Cached {
        content: content.to_owned(),
        bounds: Rectangle::with_size(Size::new(40.0, 20.0)),
        color: Color::from_rgba(0.2, 0.4, 0.6, 0.5),
        size: Pixels(16.0),
        line_height: Pixels(20.0),
        font: Font::DEFAULT,
        align_x: core::text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: core::text::Shaping::Basic,
        wrapping: core::text::Wrapping::None,
        ellipsis: core::text::Ellipsis::None,
        clip_bounds,
    }
}

fn group(item: &text::Item) -> (Transformation, &[Text]) {
    match item {
        text::Item::Group {
            transformation,
            text,
        } => (*transformation, text),
        text::Item::Cached { .. } => panic!("expected uncached text group"),
    }
}

#[test]
fn merge_coalesces_equal_transformations_and_preserves_text_clips() {
    let clip = Rectangle::with_size(Size::new(100.0, 80.0));
    let texts = vec![
        text("first", clip),
        text(
            "second",
            Rectangle::new(Point::new(10.0, 5.0), Size::new(30.0, 20.0)),
        ),
        text("third", clip),
        text("fourth", clip),
    ];
    let transformation = Transformation::translate(5.0, 8.0) * Transformation::scale(1.5);
    let mut target = Layer::with_bounds(clip);
    target.draw_text_group(vec![texts[0].clone()], transformation);
    let mut donor = Layer::with_bounds(clip);
    // Exercise adjacent groups already recorded in the incoming batch too.
    for section in &texts[1..] {
        donor.text.push(text::Item::Group {
            text: vec![section.clone()],
            transformation: Transformation::translate(5.0, 8.0) * Transformation::scale(1.5),
        });
    }
    let capacity = donor.text.capacity();

    target.merge(&mut donor);

    assert_eq!(target.text.len(), 1);
    assert_eq!(group(&target.text[0]), (transformation, texts.as_slice()));
    assert!(donor.is_empty());
    assert_eq!(donor.text.capacity(), capacity);
}

#[test]
fn merge_preserves_exact_transformation_boundaries() {
    let transformations = [
        Transformation::IDENTITY,
        Transformation::translate(f32::EPSILON, 0.0),
        Transformation::scale(2.0),
        Transformation::IDENTITY,
    ];
    let texts: Vec<_> = ["first", "second", "third", "fourth"]
        .into_iter()
        .map(|content| text(content, Rectangle::INFINITE))
        .collect();
    let mut target = Layer::default();
    target.draw_text_group(vec![texts[0].clone()], transformations[0]);
    let mut donor = Layer::default();
    for (section, transformation) in texts[1..].iter().zip(&transformations[1..]) {
        donor.draw_text_group(vec![section.clone()], *transformation);
    }

    target.merge(&mut donor);

    assert_eq!(target.text.len(), transformations.len());
    for ((item, transformation), section) in target.text.iter().zip(transformations).zip(&texts) {
        assert_eq!(group(item), (transformation, std::slice::from_ref(section)));
    }
    assert!(donor.is_empty());
}

#[test]
fn merge_preserves_cached_text_barriers_on_both_sides() {
    let texts: Vec<_> = ["before", "middle left", "middle right", "after", "last"]
        .into_iter()
        .map(|content| text(content, Rectangle::INFINITE))
        .collect();
    let cache = text::Cache::new(
        graphics::cache::Group::unique(),
        vec![text("cached", Rectangle::INFINITE)],
    )
    .expect("nonempty text cache");
    let mut target = Layer::default();
    target.draw_text_group(vec![texts[0].clone()], Transformation::IDENTITY);
    target.draw_text_cache(cache.clone(), Transformation::IDENTITY);
    target.draw_text_group(vec![texts[1].clone()], Transformation::IDENTITY);
    let mut donor = Layer::default();
    donor.draw_text_group(vec![texts[2].clone()], Transformation::IDENTITY);
    donor.draw_text_cache(cache, Transformation::IDENTITY);
    donor.draw_text_group(vec![texts[3].clone()], Transformation::IDENTITY);
    donor.draw_text_group(vec![texts[4].clone()], Transformation::IDENTITY);

    target.merge(&mut donor);

    assert_eq!(target.text.len(), 5);
    assert_eq!(group(&target.text[0]).1, &texts[..1]);
    assert_eq!(group(&target.text[2]).1, &texts[1..3]);
    assert_eq!(group(&target.text[4]).1, &texts[3..]);
    for index in [1, 3] {
        assert!(matches!(
            &target.text[index],
            text::Item::Cached { transformation, .. }
                if *transformation == Transformation::IDENTITY
        ));
    }
    assert!(donor.is_empty());
}

#[test]
fn repeated_merges_allow_emptied_donors_to_be_reused() {
    let mut target = Layer::default();
    let mut donor = Layer::default();

    for _ in 0..2 {
        let mut expected = Vec::new();
        for content in ["first", "second", "third"] {
            let section = text(content, Rectangle::INFINITE);
            expected.push(section.clone());
            donor.draw_text_group(vec![section], Transformation::IDENTITY);
            let capacity = donor.text.capacity();

            target.merge(&mut donor);
            target.merge(&mut donor);

            assert_eq!(target.text.len(), 1);
            assert_eq!(group(&target.text[0]).1, expected);
            assert!(donor.is_empty());
            assert_eq!(donor.text.capacity(), capacity);
        }
        target.reset();
        assert!(target.is_empty());
    }
}

#[test]
fn flush_reuses_compatible_group_and_retains_pending_allocation() {
    let texts: Vec<_> = ["first", "pending", "last"]
        .into_iter()
        .map(|content| text(content, Rectangle::INFINITE))
        .collect();
    let mut initial = Vec::with_capacity(8);
    initial.push(texts[0].clone());
    let group_allocation = initial.as_ptr();
    let mut layer = Layer::default();
    layer.draw_text_group(initial, Transformation::IDENTITY);
    layer.pending_text.reserve(8);
    let pending_capacity = layer.pending_text.capacity();
    let pending_allocation = layer.pending_text.as_ptr();
    layer.pending_text.push(texts[1].clone());

    // Drawing a group first flushes any pending text, preserving draw order.
    layer.draw_text_group(vec![texts[2].clone()], Transformation::IDENTITY);

    assert_eq!(layer.text.len(), 1);
    let (_, actual) = group(&layer.text[0]);
    assert_eq!(actual, texts);
    assert_eq!(actual.as_ptr(), group_allocation);
    assert!(layer.pending_text.is_empty());
    assert_eq!(layer.pending_text.capacity(), pending_capacity);
    assert_eq!(layer.pending_text.as_ptr(), pending_allocation);
}

#[test]
fn flush_preserves_cached_and_transformed_group_barriers() {
    let first = text("first", Rectangle::INFINITE);
    let pending = text("pending", Rectangle::INFINITE);
    let cache = text::Cache::new(graphics::cache::Group::unique(), vec![first.clone()])
        .expect("nonempty text cache");

    for barrier in [
        text::Item::Cached {
            cache,
            transformation: Transformation::IDENTITY,
        },
        text::Item::Group {
            text: vec![first],
            transformation: Transformation::translate(1.0, 0.0),
        },
    ] {
        let mut layer = Layer::default();
        layer.text.push(barrier);
        layer.pending_text.push(pending.clone());

        layer.flush();

        assert_eq!(layer.text.len(), 2);
        assert_eq!(
            group(&layer.text[1]),
            (Transformation::IDENTITY, std::slice::from_ref(&pending))
        );
        assert!(layer.pending_text.is_empty());
    }
}

#[test]
fn stack_merge_preserves_layer_clipping_boundaries() {
    let outer = Rectangle::with_size(Size::new(100.0, 80.0));
    let inner = Rectangle::new(Point::new(10.0, 5.0), Size::new(30.0, 20.0));
    let first = text("outer", outer);
    let second = text("inner", inner);
    let mut stack = Stack::new();
    stack.reset(outer);
    stack
        .current_mut()
        .0
        .draw_text_group(vec![first.clone()], Transformation::IDENTITY);
    stack.push_clip(inner);
    stack
        .current_mut()
        .0
        .draw_text_group(vec![second.clone()], Transformation::IDENTITY);
    stack.pop_clip();

    stack.merge();

    let layers = stack.as_slice();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].bounds, outer);
    assert_eq!(layers[1].bounds, inner);
    assert_eq!(group(&layers[0].text[0]).1, &[first]);
    assert_eq!(group(&layers[1].text[0]).1, &[second]);
}
