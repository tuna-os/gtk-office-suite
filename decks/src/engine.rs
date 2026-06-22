// engine.rs — Decks presentation engine.
use std::path::Path;

pub struct Deck {
    pub slides: Vec<Slide>,
}
pub struct Slide {
    pub title: String,
    pub objects: Vec<SlideObject>,
}
#[allow(dead_code)]
pub enum SlideObject {
    TextBox {
        text: String,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
}

impl Deck {
    pub fn new() -> Self {
        Self {
            slides: vec![
                Slide {
                    title: "Slide 1".into(),
                    objects: vec![
                        SlideObject::TextBox {
                            text: "### Welcome to Decks".into(),
                            x: 50.0,
                            y: 50.0,
                            w: 800.0,
                            h: 60.0,
                        },
                        SlideObject::TextBox {
                            text: "This is a native **Rust + GTK4** presentation tool.".into(),
                            x: 50.0,
                            y: 150.0,
                            w: 800.0,
                            h: 50.0,
                        },
                        SlideObject::TextBox {
                            text: "Use *Markdown* formatting natively inside text boxes!".into(),
                            x: 50.0,
                            y: 220.0,
                            w: 800.0,
                            h: 50.0,
                        },
                        SlideObject::Rect {
                            x: 50.0,
                            y: 320.0,
                            w: 760.0,
                            h: 120.0,
                        },
                    ],
                },
                Slide {
                    title: "Slide 2".into(),
                    objects: vec![
                        SlideObject::TextBox {
                            text: "### Second Slide".into(),
                            x: 50.0,
                            y: 50.0,
                            w: 800.0,
                            h: 60.0,
                        },
                        SlideObject::TextBox {
                            text: "This slide was dynamically generated!".into(),
                            x: 50.0,
                            y: 150.0,
                            w: 800.0,
                            h: 50.0,
                        },
                    ],
                },
            ],
        }
    }
    pub fn add_slide(&mut self) {
        self.slides.push(Slide {
            title: format!("Slide {}", self.slides.len() + 1),
            objects: vec![SlideObject::TextBox {
                text: "### New Slide".into(),
                x: 50.0,
                y: 50.0,
                w: 800.0,
                h: 60.0,
            }],
        });
    }
    #[allow(dead_code)]
    pub fn delete_slide(&mut self, idx: usize) {
        if idx < self.slides.len() {
            self.slides.remove(idx);
        }
    }
}

#[allow(dead_code)]
pub fn read_pptx(path: &Path) -> Result<Deck, String> {
    Err(format!(
        "ppt-rs read: {} — call ppt_rs::read",
        path.display()
    ))
}
#[allow(dead_code)]
pub fn write_pptx(path: &Path, _deck: &Deck) -> Result<(), String> {
    Err(format!("ppt-rs write: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_add_slide() {
        let mut d = Deck::new();
        d.add_slide();
        assert_eq!(d.slides.len(), 3);
    }
    #[test]
    fn test_delete_slide() {
        let mut d = Deck::new();
        d.add_slide();
        d.delete_slide(1);
        assert_eq!(d.slides.len(), 2);
    }
}
