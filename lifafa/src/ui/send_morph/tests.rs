use super::draw::{around, ease};
use super::measure::Measured;
use gtk::graphene;

#[test]
fn the_curve_starts_and_ends_at_rest() {
    assert_eq!(ease(0.0), 0.0);
    assert_eq!(ease(1.0), 1.0);
    let mut last = 0.0;
    for i in 1..100 {
        let v = ease(i as f64 / 100.0);
        assert!(v >= last, "the curve never goes back");
        last = v;
    }
}

#[test]
fn a_hole_leaves_four_strips() {
    let outer = graphene::Rect::new(0.0, 0.0, 100.0, 100.0);
    let hole = graphene::Rect::new(10.0, 20.0, 80.0, 50.0);
    let strips = around(&outer, &hole);
    let area: f32 = strips.iter().map(|r| r.width() * r.height()).sum();
    assert_eq!(area, 100.0 * 100.0 - 80.0 * 50.0);
}

#[test]
fn measured_cards_land_in_stage_pixels() {
    let view = graphene::Rect::new(10.0, 100.0, 400.0, 300.0);
    let m = Measured::parse(
        r#"{"w":200,"y":0,"live":true,"cards":[["1:2",7,7,186,50,20]]}"#,
        &view,
    )
    .unwrap();
    let c = &m.cards()[0];
    assert_eq!(c.key, "1:2");
    assert_eq!(
        (c.rect.x(), c.rect.y(), c.rect.width(), c.head),
        (24.0, 114.0, 372.0, 40.0)
    );
}
