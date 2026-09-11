// stateful.rs — seeded command sequences against DecksController (#442).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The third app to get the treatment, after tables-core and letters-core.
// A deck is a list of slides holding a list of objects, and almost every
// command here is an index into one of those lists — add, delete,
// reorder, align, restack — which is the shape of code where an
// out-of-date index survives every test written one command at a time.
//
// The generator and minimizer are duplicated from the other two on
// purpose; the reasoning is written out in tables-core/tests/stateful.rs
// and holds here: a seed in a failure message has to reproduce the run
// for anyone, forever, so the PRNG cannot be a dependency that might
// change its stream under a version bump.

use decks_core::undo::{AlignMode, DistributeMode, ZOrderOp};
use decks_core::{DecksController, Slide, SlideObject};

/// Width and height are not accessors on SlideObject the way x, y and
/// rotation are — a circle stores a radius — so the invariant does the
/// conversion once, here.
fn size(object: &SlideObject) -> (f64, f64) {
    match object {
        SlideObject::TextBox { w, h, .. }
        | SlideObject::Rect { w, h, .. }
        | SlideObject::Image { w, h, .. } => (*w, *h),
        SlideObject::Circle { r, .. } => (*r * 2.0, *r * 2.0),
    }
}

const STEPS: usize = 200;

/// Seeds that run on every pull request. A campaign that finds a failing
/// seed adds it here, so the regression is permanent rather than
/// something the nightly might happen to hit again.
const FIXED_SEEDS: &[u64] = &[
    1, 2, 3, 7, 11, 20260907, 20260911, 0xDEADBEEF,
    0x5EED, 42, 99, 1337, 8675309, 0xC0FFEE, 314159, 271828,
];

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0x1234_5678))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0 | 1;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as usize
    }

    fn coord(&mut self) -> f64 {
        (self.below(960)) as f64
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Command {
    AddSlide(usize),
    DeleteSlide(usize),
    MoveSlideUp(usize),
    MoveSlideDown(usize),
    AddTextBox { slide: usize, text: String },
    AddRect { slide: usize },
    AddCircle { slide: usize },
    DeleteObject { slide: usize, index: usize },
    MoveObject { slide: usize, index: usize, dx: f64, dy: f64 },
    ResizeObject { slide: usize, index: usize, w: f64, h: f64 },
    RotateObject { slide: usize, index: usize, angle: f64 },
    ChangeText { slide: usize, index: usize, text: String },
    Align { slide: usize, mode: AlignMode },
    Distribute { slide: usize, mode: DistributeMode },
    ZOrder { slide: usize, index: usize, op: ZOrderOp },
    Undo,
    UndoThenRedo,
}

fn object_count(controller: &DecksController, slide: usize) -> usize {
    controller
        .slides
        .borrow()
        .get(slide)
        .map(|s| s.objects.len())
        .unwrap_or(0)
}

fn generate(rng: &mut Rng, controller: &DecksController) -> Command {
    let slide_count = controller.slide_count().max(1);
    let slide = rng.below(slide_count);
    let objects = object_count(controller, slide);
    let index = rng.below(objects.max(1));

    match rng.below(24) {
        0..=1 => Command::AddSlide(rng.below(slide_count + 1)),
        2 => Command::DeleteSlide(slide),
        3 => Command::MoveSlideUp(slide),
        4 => Command::MoveSlideDown(slide),
        5..=7 => Command::AddTextBox {
            slide,
            // Styled runs are what makes a text box interesting: the
            // model promises concatenated run text equals `text`.
            text: format!("slide text {}", rng.below(100)),
        },
        8 => Command::AddRect { slide },
        9 => Command::AddCircle { slide },
        10 => Command::DeleteObject { slide, index },
        11..=12 => Command::MoveObject {
            slide,
            index,
            dx: rng.coord() - 480.0,
            dy: rng.coord() - 480.0,
        },
        13 => Command::ResizeObject {
            slide,
            index,
            w: 1.0 + rng.coord(),
            h: 1.0 + rng.coord(),
        },
        14 => Command::RotateObject {
            slide,
            index,
            angle: rng.below(360) as f64,
        },
        15..=16 => Command::ChangeText {
            slide,
            index,
            text: format!("edited {}", rng.below(100)),
        },
        17..=18 => Command::Align {
            slide,
            mode: match rng.below(6) {
                0 => AlignMode::Left,
                1 => AlignMode::Center,
                2 => AlignMode::Right,
                3 => AlignMode::Top,
                4 => AlignMode::Middle,
                _ => AlignMode::Bottom,
            },
        },
        19 => Command::Distribute {
            slide,
            mode: if rng.below(2) == 0 {
                DistributeMode::Horizontal
            } else {
                DistributeMode::Vertical
            },
        },
        20..=21 => Command::ZOrder {
            slide,
            index,
            op: match rng.below(4) {
                0 => ZOrderOp::BringToFront,
                1 => ZOrderOp::SendToBack,
                2 => ZOrderOp::BringForward,
                _ => ZOrderOp::SendBackward,
            },
        },
        22 => Command::Undo,
        _ => Command::UndoThenRedo,
    }
}

fn blank_slide() -> Slide {
    Slide {
        title: String::new(),
        background: String::new(),
        objects: Vec::new(),
        notes: String::new(),
        master_idx: None,
    }
}

fn text_box(text: &str, x: f64, y: f64) -> SlideObject {
    SlideObject::TextBox {
        text: text.to_string(),
        x,
        y,
        w: 200.0,
        h: 60.0,
        rotation: 0.0,
        runs: Vec::new(),
    }
}

fn object_at(controller: &DecksController, slide: usize, index: usize) -> Option<SlideObject> {
    controller
        .slides
        .borrow()
        .get(slide)
        .and_then(|s| s.objects.get(index))
        .cloned()
}

fn apply(controller: &DecksController, command: &Command) -> Result<(), String> {
    match command {
        Command::AddSlide(index) => {
            controller.add_slide(*index, blank_slide());
        }
        Command::DeleteSlide(index) => {
            controller.delete_slide(*index);
        }
        Command::MoveSlideUp(index) => {
            controller.move_slide_up(*index);
        }
        Command::MoveSlideDown(index) => {
            controller.move_slide_down(*index);
        }
        Command::AddTextBox { slide, text } => {
            controller.add_object(*slide, text_box(text, 10.0, 10.0));
        }
        Command::AddRect { slide } => {
            controller.add_object(
                *slide,
                SlideObject::Rect { x: 20.0, y: 20.0, w: 100.0, h: 80.0, rotation: 0.0 },
            );
        }
        Command::AddCircle { slide } => {
            controller.add_object(
                *slide,
                SlideObject::Circle { x: 50.0, y: 50.0, r: 30.0, rotation: 0.0 },
            );
        }
        Command::DeleteObject { slide, index } => {
            if let Some(object) = object_at(controller, *slide, *index) {
                controller.delete_object(*slide, *index, object);
            }
        }
        Command::MoveObject { slide, index, dx, dy } => {
            controller.move_object(*slide, *index, *dx, *dy);
        }
        Command::ResizeObject { slide, index, w, h } => {
            if let Some(object) = object_at(controller, *slide, *index) {
                let (width, height) = size(&object);
                let old = (object.x(), object.y(), width, height);
                controller.resize_object(*slide, *index, old, (old.0, old.1, *w, *h));
            }
        }
        Command::RotateObject { slide, index, angle } => {
            if let Some(object) = object_at(controller, *slide, *index) {
                controller.rotate_object(*slide, *index, object.rotation(), *angle);
            }
        }
        Command::ChangeText { slide, index, text } => {
            if let Some(SlideObject::TextBox { text: old, .. }) =
                object_at(controller, *slide, *index)
            {
                controller.change_text(*slide, *index, old, text.clone());
            }
        }
        Command::Align { slide, mode } => {
            let indices: Vec<usize> = (0..object_count(controller, *slide)).collect();
            controller.align_objects(*slide, &indices, *mode);
        }
        Command::Distribute { slide, mode } => {
            let indices: Vec<usize> = (0..object_count(controller, *slide)).collect();
            controller.distribute_objects(*slide, &indices, *mode);
        }
        Command::ZOrder { slide, index, op } => {
            controller.z_order_object(*slide, *index, *op);
        }
        Command::Undo => {
            controller.undo();
        }
        Command::UndoThenRedo => {
            let before = document(controller);
            if controller.undo() {
                if !controller.redo() {
                    return Err("redo refused immediately after a successful undo".into());
                }
                let after = document(controller);
                if before != after {
                    return Err(format!(
                        "undo followed by redo changed the deck\n  before: {before}\n  after:  {after}"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The deck as text, for comparing a state against itself across an
/// undo/redo pair. Geometry is included — unlike the Tables harness,
/// where the selection is view state, everything here is the document.
fn document(controller: &DecksController) -> String {
    let slides = controller.slides.borrow();
    slides
        .iter()
        .map(|slide| {
            let objects: Vec<String> = slide
                .objects
                .iter()
                .map(|object| {
                    format!(
                        "{}({:.3},{:.3},{:.3},{:.3},{:.3}){:?}",
                        kind(object),
                        object.x(),
                        object.y(),
                        size(object).0,
                        size(object).1,
                        object.rotation(),
                        text_of(object),
                    )
                })
                .collect();
            format!("[{}|{}]", slide.title, objects.join(","))
        })
        .collect::<Vec<_>>()
        .join("")
}

fn kind(object: &SlideObject) -> &'static str {
    match object {
        SlideObject::TextBox { .. } => "T",
        SlideObject::Rect { .. } => "R",
        SlideObject::Circle { .. } => "C",
        SlideObject::Image { .. } => "I",
    }
}

fn text_of(object: &SlideObject) -> Option<&str> {
    match object {
        SlideObject::TextBox { text, .. } => Some(text.as_str()),
        _ => None,
    }
}

// ── invariants ───────────────────────────────────────────────────────

fn check_invariants(controller: &DecksController) -> Result<(), String> {
    let slides = controller.slides.borrow();

    for (slide_index, slide) in slides.iter().enumerate() {
        for (index, object) in slide.objects.iter().enumerate() {
            let where_ = format!("slide {slide_index}, object {index}");

            // Geometry that is not a number renders as nothing and
            // exports as garbage, and it propagates: one NaN in an align
            // or distribute takes the whole row with it.
            let (width, height) = size(object);
            for (name, value) in [
                ("x", object.x()),
                ("y", object.y()),
                ("w", width),
                ("h", height),
                ("rotation", object.rotation()),
            ] {
                if !value.is_finite() {
                    return Err(format!("{where_}: {name} is {value}"));
                }
            }

            if width < 0.0 || height < 0.0 {
                return Err(format!("{where_}: negative size {width}x{height}"));
            }

            // The model's own promise, from engine/model.rs: "Styled runs
            // (shared WYSIWYG primitive with Letters). When non-empty,
            // concatenated run text equals `text`." A renderer that draws
            // the runs and a model that stores the text disagree the
            // moment that stops being true.
            if let SlideObject::TextBox { text, runs, .. } = object {
                if !runs.is_empty() {
                    let from_runs: String = runs.iter().map(|r| r.text.as_str()).collect();
                    if &from_runs != text {
                        return Err(format!(
                            "{where_}: runs say {from_runs:?} but text says {text:?}"
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

// ── running and minimizing ───────────────────────────────────────────

fn fresh() -> DecksController {
    DecksController::new(vec![blank_slide()], Vec::new())
}

fn replay(commands: &[Command]) -> Result<(), (usize, String)> {
    let controller = fresh();
    for (index, command) in commands.iter().enumerate() {
        if let Err(reason) = apply(&controller, command) {
            return Err((index, reason));
        }
        if let Err(reason) = check_invariants(&controller) {
            return Err((index, reason));
        }
    }
    Ok(())
}

fn minimize(commands: &[Command], failing_at: usize) -> Vec<Command> {
    let mut shortest = commands[..=failing_at].to_vec();
    let mut index = 0;
    while index < shortest.len() {
        let mut candidate = shortest.clone();
        candidate.remove(index);
        if replay(&candidate).is_err() {
            shortest = candidate;
        } else {
            index += 1;
        }
    }
    shortest
}

fn run_seed(seed: u64, steps: usize) -> Result<(), String> {
    let mut rng = Rng::new(seed);
    let controller = fresh();
    let mut trace = Vec::with_capacity(steps);

    for step in 0..steps {
        let command = generate(&mut rng, &controller);
        trace.push(command.clone());
        let outcome = apply(&controller, &command)
            .and_then(|()| check_invariants(&controller));
        if let Err(reason) = outcome {
            let minimal = minimize(&trace, step);
            // Report the *minimized* trace's own reason. Dropping
            // commands can leave a shorter sequence that fails a
            // different way, and printing the original run's message
            // beside it sends the reader looking for a symptom the
            // listed commands do not produce.
            let reason = match replay(&minimal) {
                Err((_, minimal_reason)) => minimal_reason,
                Ok(()) => reason,
            };
            return Err(format!(
                "seed {seed} failed at step {step}: {reason}\n\
                 minimized to {} command(s):\n{}",
                minimal.len(),
                minimal
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("  {i}: {c:?}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ));
        }
    }
    Ok(())
}

// ── tests ────────────────────────────────────────────────────────────

#[test]
fn fixed_seeds_hold_the_invariants() {
    let mut failures = Vec::new();
    for &seed in FIXED_SEEDS {
        if let Err(report) = run_seed(seed, STEPS) {
            failures.push(report);
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} seed(s) failed:\n\n{}",
        failures.len(),
        FIXED_SEEDS.len(),
        failures.join("\n\n")
    );
}

#[test]
fn a_seed_replays_identically() {
    let commands = |seed: u64| {
        let mut rng = Rng::new(seed);
        let controller = fresh();
        (0..16).map(|_| generate(&mut rng, &controller)).collect::<Vec<_>>()
    };
    assert_eq!(commands(20260911), commands(20260911));
    assert_ne!(commands(1), commands(2));
}

#[test]
fn the_run_invariant_rejects_a_text_box_that_disagrees_with_itself() {
    // An invariant that cannot fail reads exactly like one that never
    // fires, so this shows the check catching what it is there for.
    let controller = fresh();
    controller.add_object(0, text_box("hello", 0.0, 0.0));
    assert!(check_invariants(&controller).is_ok());

    {
        let mut slides = controller.slides.borrow_mut();
        if let Some(SlideObject::TextBox { runs, .. }) = slides[0].objects.get_mut(0) {
            runs.push(letters_core::model::Run::plain("goodbye"));
        }
    }

    let error = check_invariants(&controller).expect_err("a disagreeing text box is rejected");
    assert!(error.contains("runs say"), "unexpected rejection: {error}");
}

/// The nightly campaign's entry point. Ignored by default so the PR lane
/// stays fast; run with
/// `cargo test -p decks-core --test stateful -- --ignored`.
#[test]
#[ignore = "campaign-scale; run from the nightly stress workflow"]
fn seed_campaign() {
    let base: u64 = std::env::var("STATEFUL_SEED_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000);
    let count: u64 = std::env::var("STATEFUL_SEED_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);

    let mut failures = Vec::new();
    for seed in base..base + count {
        if let Err(report) = run_seed(seed, STEPS) {
            failures.push(report);
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {count} seed(s) failed — add each failing seed to FIXED_SEEDS \
         once its bug is fixed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
