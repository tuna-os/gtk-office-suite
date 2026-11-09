//! Tidying the walked tree: flattening layout boxes, wrapping loose inline
//! runs, merging text and turning double breaks into paragraphs.

use super::{Elem, Out};

/// Turn the raw walk into something that reads: empty boxes go, runs of
/// inline content become paragraphs, a double line break is a paragraph
/// break, and a box holding a single box is that box.
pub(super) fn tidy(kids: Vec<Out>, root: bool) -> Vec<Out> {
    let mut out = Vec::with_capacity(kids.len());
    for k in kids {
        match k {
            Out::Text(t) => out.push(Out::Text(t)),
            Out::Elem(mut e) => {
                if matches!(e.tag, "br" | "hr" | "img") {
                    out.push(Out::Elem(e));
                    continue;
                }
                if e.keeps_space() {
                    // Shown as written; only an entirely blank one goes.
                    let node = Out::Elem(e);
                    if node.has_content() {
                        out.push(node);
                    }
                    continue;
                }
                e.kids = tidy(std::mem::take(&mut e.kids), false);
                if matches!(
                    e.tag,
                    "div" | "blockquote" | "li" | "td" | "th" | "dd" | "figure"
                ) {
                    e.kids = paragraphs(std::mem::take(&mut e.kids), e.tag != "div");
                }
                // A heading (or paragraph) holds a line, not boxes: blocks a
                // sender nested in one are opened up, a line break between.
                if matches!(e.tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p")
                    && e.kids.iter().any(Out::is_block)
                {
                    e.kids = trim_run(flatten_blocks(std::mem::take(&mut e.kids)));
                }
                let node = Out::Elem(e);
                if !node.has_content() {
                    continue;
                }
                match node {
                    // A box that only holds boxes is structure, not content:
                    // its boxes stand on their own. (Only a marked one — a
                    // quote, a signature — keeps its wrapper.)
                    Out::Elem(e) if e.tag == "div" && e.attrs.is_empty() => out.extend(e.kids),
                    // Inline formatting around blocks (a link around a whole
                    // card) is turned inside out: the formatting goes into
                    // each block, and the blocks stand on their own.
                    Out::Elem(e) if !e.is_block() && e.kids.iter().any(Out::is_block) => {
                        let shell = Elem {
                            tag: e.tag,
                            attrs: e.attrs.clone(),
                            kids: Vec::new(),
                        };
                        out.extend(wrap_inline(&shell, e.kids));
                    }
                    n => out.push(n),
                }
            }
        }
    }
    let out = merge_text(out);
    if root {
        paragraphs(out, false)
    } else {
        out
    }
}

/// Every block among `kids` replaced by its content, with a line break
/// where one block met the next.
fn flatten_blocks(kids: Vec<Out>) -> Vec<Out> {
    let mut out = Vec::new();
    for k in kids {
        match k {
            Out::Elem(e) if e.is_block() => {
                if e.tag == "hr" {
                    continue;
                }
                if out.iter().any(Out::has_content) {
                    out.push(Out::Elem(Elem::new("br")));
                }
                out.extend(flatten_blocks(e.kids));
            }
            k => out.push(k),
        }
    }
    out
}

/// `kids` wrapped in a copy of `shell` (an inline element) — block by block
/// where they hold blocks, so no block ever sits inside an inline element.
fn wrap_inline(shell: &Elem, kids: Vec<Out>) -> Vec<Out> {
    let clone = |kids: Vec<Out>| {
        Out::Elem(Elem {
            tag: shell.tag,
            attrs: shell.attrs.clone(),
            kids,
        })
    };
    if !kids.iter().any(Out::is_block) {
        return vec![clone(kids)];
    }
    let mut out = Vec::new();
    let mut run = Vec::new();
    for k in kids {
        match k {
            Out::Elem(mut e) if e.is_block() => {
                if run.iter().any(Out::has_content) {
                    out.push(clone(std::mem::take(&mut run)));
                } else {
                    run.clear();
                }
                if e.tag != "hr" {
                    e.kids = wrap_inline(shell, std::mem::take(&mut e.kids));
                }
                out.push(Out::Elem(e));
            }
            k => run.push(k),
        }
    }
    if run.iter().any(Out::has_content) {
        out.push(clone(run));
    }
    out
}

/// Adjacent text runs joined, so later passes see one string.
fn merge_text(kids: Vec<Out>) -> Vec<Out> {
    let mut out: Vec<Out> = Vec::with_capacity(kids.len());
    for k in kids {
        match (out.last_mut(), k) {
            (Some(Out::Text(prev)), Out::Text(t)) => prev.push_str(&t),
            (_, k) => out.push(k),
        }
    }
    out
}

/// Wrap runs of inline content between blocks into paragraphs, splitting a
/// run at a double line break. With `keep_lone`, a list of one inline run
/// stays unwrapped (an `li` or a cell reads better without a `p` inside).
fn paragraphs(kids: Vec<Out>, keep_lone: bool) -> Vec<Out> {
    let has_block = kids.iter().any(Out::is_block);
    if keep_lone && !has_block {
        return trim_run(kids);
    }
    let mut out = Vec::new();
    let mut run: Vec<Out> = Vec::new();
    let flush = |run: &mut Vec<Out>, out: &mut Vec<Out>| {
        for piece in split_breaks(std::mem::take(run)) {
            let piece = trim_run(piece);
            if piece.iter().any(Out::has_content) {
                out.push(Out::Elem(Elem {
                    tag: "p",
                    attrs: Vec::new(),
                    kids: piece,
                }));
            }
        }
    };
    for k in kids {
        if k.is_block() {
            flush(&mut run, &mut out);
            out.push(k);
        } else {
            run.push(k);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Split an inline run at two or more consecutive line breaks (blank text
/// between them notwithstanding).
fn split_breaks(run: Vec<Out>) -> Vec<Vec<Out>> {
    let mut pieces = Vec::new();
    let mut piece = Vec::new();
    let mut it = run.into_iter().peekable();
    while let Some(item) = it.next() {
        if item.is_br() {
            let mut gap = vec![item];
            let mut breaks = 1;
            while let Some(n) = it.next_if(|n| n.is_br() || n.is_blank_text()) {
                if n.is_br() {
                    breaks += 1;
                }
                gap.push(n);
            }
            if breaks >= 2 {
                pieces.push(std::mem::take(&mut piece));
            } else {
                piece.extend(gap);
            }
            continue;
        }
        piece.push(item);
    }
    pieces.push(piece);
    pieces
}

/// Drop blank text and line breaks at both ends of a run, and trim the
/// leading space of its first text and trailing space of its last.
fn trim_run(mut run: Vec<Out>) -> Vec<Out> {
    while run.first().is_some_and(|k| k.is_br() || k.is_blank_text()) {
        run.remove(0);
    }
    while run.last().is_some_and(|k| k.is_br() || k.is_blank_text()) {
        run.pop();
    }
    if let Some(Out::Text(t)) = run.first_mut() {
        *t = t.trim_start().to_string();
    }
    if let Some(Out::Text(t)) = run.last_mut() {
        *t = t.trim_end().to_string();
    }
    run
}
