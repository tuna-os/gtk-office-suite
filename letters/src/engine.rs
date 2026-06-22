// engine.rs — Letters document engine.
use gtk::prelude::*;
use gtk4 as gtk;

#[allow(dead_code)]
pub struct Document {
    pub text: String,
}
#[allow(dead_code)]
impl Document {
    pub fn new() -> Self {
        Self {
            text: String::new(),
        }
    }
    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

#[allow(dead_code)]
pub fn markdown_to_html(md: &str) -> String {
    use pulldown_cmark::{html, Parser};
    let parser = Parser::new(md);
    let mut buf = String::new();
    html::push_html(&mut buf, parser);
    buf
}

#[allow(dead_code)]
pub fn parse_markdown_into_buffer(md: &str, buffer: &gtk::TextBuffer) {
    buffer.set_text("");
    let parser = pulldown_cmark::Parser::new(md);

    let mut current_tags = Vec::<String>::new();

    for event in parser {
        match event {
            pulldown_cmark::Event::Start(tag) => match tag {
                pulldown_cmark::Tag::Heading { level, .. } => {
                    let tag_name = match level {
                        pulldown_cmark::HeadingLevel::H1 => "h1",
                        pulldown_cmark::HeadingLevel::H2 => "h2",
                        _ => "h3",
                    };
                    current_tags.push(tag_name.to_string());
                }
                pulldown_cmark::Tag::BlockQuote(_) => {
                    current_tags.push("quote".to_string());
                }
                pulldown_cmark::Tag::Strong => {
                    current_tags.push("bold".to_string());
                }
                pulldown_cmark::Tag::Emphasis => {
                    current_tags.push("italic".to_string());
                }
                pulldown_cmark::Tag::Item => {
                    let mut end_iter = buffer.end_iter();
                    buffer.insert(&mut end_iter, "• ");
                }
                _ => {}
            },
            pulldown_cmark::Event::End(tag) => match tag {
                pulldown_cmark::TagEnd::Heading(level) => {
                    let tag_name = match level {
                        pulldown_cmark::HeadingLevel::H1 => "h1",
                        pulldown_cmark::HeadingLevel::H2 => "h2",
                        _ => "h3",
                    };
                    current_tags.retain(|x| x != tag_name);
                    let mut end_iter = buffer.end_iter();
                    buffer.insert(&mut end_iter, "\n");
                }
                pulldown_cmark::TagEnd::BlockQuote(_) => {
                    current_tags.retain(|x| x != "quote");
                    let mut end_iter = buffer.end_iter();
                    buffer.insert(&mut end_iter, "\n");
                }
                pulldown_cmark::TagEnd::Strong => {
                    current_tags.retain(|x| x != "bold");
                }
                pulldown_cmark::TagEnd::Emphasis => {
                    current_tags.retain(|x| x != "italic");
                }
                pulldown_cmark::TagEnd::Item => {
                    let mut end_iter = buffer.end_iter();
                    buffer.insert(&mut end_iter, "\n");
                }
                _ => {}
            },
            pulldown_cmark::Event::Text(text) => {
                let start_offset = buffer.char_count();
                let end_iter = buffer.end_iter();
                buffer.insert(&mut end_iter.clone(), &text);

                let start_iter = buffer.iter_at_offset(start_offset);
                let end_iter_val = buffer.end_iter();

                for tag_name in &current_tags {
                    buffer.apply_tag_by_name(tag_name, &start_iter, &end_iter_val);
                }
            }
            pulldown_cmark::Event::Code(text) => {
                let start_offset = buffer.char_count();
                let end_iter = buffer.end_iter();
                buffer.insert(&mut end_iter.clone(), &text);

                let start_iter = buffer.iter_at_offset(start_offset);
                let end_iter_val = buffer.end_iter();

                buffer.apply_tag_by_name("code", &start_iter, &end_iter_val);
                for tag_name in &current_tags {
                    buffer.apply_tag_by_name(tag_name, &start_iter, &end_iter_val);
                }
            }
            pulldown_cmark::Event::SoftBreak | pulldown_cmark::Event::HardBreak => {
                let mut end_iter = buffer.end_iter();
                buffer.insert(&mut end_iter, "\n");
            }
            _ => {}
        }
    }
}

#[allow(dead_code)]
pub fn serialize_buffer_to_markdown(buffer: &gtk::TextBuffer) -> String {
    let mut md = String::new();
    let line_count = buffer.line_count();
    for i in 0..line_count {
        let start = buffer.iter_at_line(i).unwrap();
        let mut end = start;
        end.forward_to_line_end();

        let line_text = buffer.text(&start, &end, false).to_string();
        if line_text.is_empty() {
            md.push('\n');
            continue;
        }

        let tags = start.tags();
        let is_h1 = tags
            .iter()
            .any(|t| t.name().map(|n| n == "h1").unwrap_or(false));
        let is_h2 = tags
            .iter()
            .any(|t| t.name().map(|n| n == "h2").unwrap_or(false));
        let is_h3 = tags
            .iter()
            .any(|t| t.name().map(|n| n == "h3").unwrap_or(false));
        let is_quote = tags
            .iter()
            .any(|t| t.name().map(|n| n == "quote").unwrap_or(false));

        let mut line_md = String::new();
        let mut run_iter = start;
        while run_iter < end {
            let mut next_run = run_iter;
            next_run.forward_to_tag_toggle(None::<&gtk::TextTag>);
            if next_run > end {
                next_run = end;
            }

            let run_text = buffer.text(&run_iter, &next_run, false).to_string();
            let run_tags = run_iter.tags();
            let is_bold = run_tags
                .iter()
                .any(|t| t.name().map(|n| n == "bold").unwrap_or(false));
            let is_italic = run_tags
                .iter()
                .any(|t| t.name().map(|n| n == "italic").unwrap_or(false));
            let is_code = run_tags
                .iter()
                .any(|t| t.name().map(|n| n == "code").unwrap_or(false));

            let mut formatted = run_text;
            if is_code {
                formatted = format!("`{}`", formatted);
            }
            if is_bold {
                formatted = format!("**{}**", formatted);
            }
            if is_italic {
                formatted = format!("*{}*", formatted);
            }

            line_md.push_str(&formatted);
            run_iter = next_run;
        }

        if is_h1 {
            line_md = format!("# {}", line_md);
        } else if is_h2 {
            line_md = format!("## {}", line_md);
        } else if is_h3 {
            line_md = format!("### {}", line_md);
        } else if is_quote {
            line_md = format!("> {}", line_md);
        }

        md.push_str(&line_md);
        md.push('\n');
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_word_count() {
        let d = Document {
            text: "hello world rust".into(),
        };
        assert_eq!(d.word_count(), 3);
    }
    #[test]
    fn test_empty() {
        let d = Document::new();
        assert_eq!(d.word_count(), 0);
    }
}
