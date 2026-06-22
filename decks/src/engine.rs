// engine.rs — Decks presentation engine.

pub struct Deck { pub slides: Vec<Slide> }
pub struct Slide { pub title: String, pub objects: Vec<SlideObject> }
pub enum SlideObject { TextBox { text: String, x: f64, y: f64, w: f64, h: f64 }, Rect { x: f64, y: f64, w: f64, h: f64 } }

impl Deck {
    pub fn new() -> Self { Self { slides: vec![Slide { title: "Slide 1".into(), objects: vec![] }] } }
    pub fn add_slide(&mut self) { self.slides.push(Slide { title: format!("Slide {}", self.slides.len() + 1), objects: vec![] }); }
    pub fn delete_slide(&mut self, idx: usize) { if idx < self.slides.len() { self.slides.remove(idx); } }
}

pub fn read_pptx(path: &Path) -> Result<Deck, String> { Err(format!("ppt-rs read: {} — call ppt_rs::read", path.display())) }
pub fn write_pptx(path: &Path, _deck: &Deck) -> Result<(), String> { Err(format!("ppt-rs write: {}", path.display())) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_add_slide() { let mut d = Deck::new(); d.add_slide(); assert_eq!(d.slides.len(), 2); }
    #[test] fn test_delete_slide() { let mut d = Deck::new(); d.add_slide(); d.delete_slide(1); assert_eq!(d.slides.len(), 1); }
}
