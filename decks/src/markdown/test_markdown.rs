use super::cairo;
use super::render_markdown;

fn render(text: &str) -> (f64, u64) {
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 320, 240).unwrap();
    let height = {
        let cr = cairo::Context::new(&surface).unwrap();
        render_markdown(&cr, text, 8.0, 8.0, 304.0, 14.0)
    };
    surface.flush();
    let checksum = surface.data().unwrap().iter().fold(0_u64, |sum, byte| {
        sum.wrapping_mul(31).wrapping_add(u64::from(*byte))
    });
    (height, checksum)
}

#[test]
fn empty_markdown_draws_nothing() {
    let (height, checksum) = render("");

    assert_eq!(height, 0.0);
    assert_eq!(checksum, 0);
}

#[test]
fn plain_text_draws_and_reports_height() {
    let (height, checksum) = render("Plain text");

    assert!(height > 0.0);
    assert_ne!(checksum, 0);
}

#[test]
fn markdown_styles_change_rendered_pixels() {
    let (_, plain) = render("Styled");
    let (_, bold) = render("**Styled**");
    let (_, italic) = render("*Styled*");
    let (_, code) = render("`Styled`");
    let (_, code_block) = render("```\nStyled\n```");

    assert_ne!(plain, bold);
    assert_ne!(plain, italic);
    assert_ne!(plain, code);
    assert_ne!(plain, code_block);
}

#[test]
fn line_breaks_add_vertical_spacing() {
    let (plain_height, _) = render("first second");
    let (soft_break_height, _) = render("first\nsecond");
    let (hard_break_height, _) = render("first  \nsecond");

    assert!(soft_break_height > plain_height);
    assert!(hard_break_height > plain_height);
}
