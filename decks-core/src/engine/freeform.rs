// SPDX-License-Identifier: GPL-3.0-or-later
//! Freeform shapes: DrawingML's custom geometry (`a:custGeom`), the paths a
//! chart pasted as shapes, an icon or a hand-drawn outline is made of, and
//! ODF's enhanced path, the same thing as Impress writes it.

/// One command of a path, in the path's own `w`×`h` space. Angles are in
/// degrees, clockwise from east, as DrawingML's `arcTo` gives them.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PathCmd {
    Move(f64, f64),
    Line(f64, f64),
    /// Two control points, then the end.
    Cubic(f64, f64, f64, f64, f64, f64),
    /// One control point, then the end.
    Quad(f64, f64, f64, f64),
    /// An arc of the ellipse with radii `wr`, `hr` the current point lies
    /// on at angle `start`, swept by `swing`.
    Arc { wr: f64, hr: f64, start: f64, swing: f64 },
    Close,
}

/// One path of a freeform shape: its commands in a `w`×`h` space stretched
/// to the shape's box, and whether it is filled and outlined.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FreePath {
    pub w: f64,
    pub h: f64,
    pub cmds: Vec<PathCmd>,
    pub fill: bool,
    pub stroke: bool,
}

/// The centre of the ellipse with radii `wr`, `hr` on which `(x, y)` lies
/// at angle `start` (degrees).
pub fn arc_centre((x, y): (f64, f64), wr: f64, hr: f64, start: f64) -> (f64, f64) {
    let a = start.to_radians();
    (x - wr * a.cos(), y - hr * a.sin())
}

/// An arc from `from` as cubic Béziers, at most a quarter turn each:
/// `(c1, c2, end)` triples. ODF's enhanced path has no arc of this kind.
pub fn arc_as_cubics(from: (f64, f64), wr: f64, hr: f64, start: f64, swing: f64) -> Vec<[(f64, f64); 3]> {
    let (cx, cy) = arc_centre(from, wr, hr, start);
    let n = (swing.abs() / 90.0).ceil().max(1.0) as usize;
    let step = (swing / n as f64).to_radians();
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |a: f64| (cx + wr * a.cos(), cy + hr * a.sin());
    let tangent = |a: f64| (-wr * a.sin(), hr * a.cos());
    (0..n)
        .map(|i| {
            let a0 = start.to_radians() + step * i as f64;
            let a1 = a0 + step;
            let (p0, p1) = (point(a0), point(a1));
            let (t0, t1) = (tangent(a0), tangent(a1));
            [(p0.0 + k * t0.0, p0.1 + k * t0.1), (p1.0 - k * t1.0, p1.1 - k * t1.1), p1]
        })
        .collect()
}

/// The paths as one ODF enhanced path over a `view`×`view` box: each path
/// scaled from its own space into the box, an arc as Béziers, "F" for a
/// path not filled and "S" for one not outlined, "N" ending each.
pub fn to_enhanced_path(paths: &[FreePath], view: f64) -> String {
    let mut out = Vec::new();
    for p in paths {
        let (sx, sy) = (view / p.w.max(1e-9), view / p.h.max(1e-9));
        let pt = |x: f64, y: f64| format!("{} {}", (x * sx).round(), (y * sy).round());
        let mut cur = (0.0, 0.0);
        let mut start = (0.0, 0.0);
        for c in &p.cmds {
            match *c {
                PathCmd::Move(x, y) => {
                    out.push(format!("M {}", pt(x, y)));
                    cur = (x, y);
                    start = cur;
                }
                PathCmd::Line(x, y) => {
                    out.push(format!("L {}", pt(x, y)));
                    cur = (x, y);
                }
                PathCmd::Cubic(a, b, c2, d, x, y) => {
                    out.push(format!("C {} {} {}", pt(a, b), pt(c2, d), pt(x, y)));
                    cur = (x, y);
                }
                PathCmd::Quad(a, b, x, y) => {
                    out.push(format!("Q {} {}", pt(a, b), pt(x, y)));
                    cur = (x, y);
                }
                PathCmd::Arc { wr, hr, start: st, swing } => {
                    for [c1, c2, e] in arc_as_cubics(cur, wr, hr, st, swing) {
                        out.push(format!("C {} {} {}", pt(c1.0, c1.1), pt(c2.0, c2.1), pt(e.0, e.1)));
                        cur = e;
                    }
                }
                PathCmd::Close => {
                    out.push("Z".into());
                    cur = start;
                }
            }
        }
        if !p.fill {
            out.push("F".into());
        }
        if !p.stroke {
            out.push("S".into());
        }
        out.push("N".into());
    }
    out.join(" ")
}

/// An ODF enhanced path over a `vw`×`vh` view box, as paths: the commands
/// this module writes (M L C Q Z, and F, S and N between paths); a path
/// using any other is not one we can draw, and `None` is returned.
pub fn from_enhanced_path(d: &str, vw: f64, vh: f64) -> Option<Vec<FreePath>> {
    let mut paths = Vec::new();
    let new = || FreePath { w: vw, h: vh, cmds: Vec::new(), fill: true, stroke: true };
    let mut cur = new();
    let tokens: Vec<&str> = d.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).collect();
    let mut i = 0;
    let mut cmd = 'M';
    let num = |i: &mut usize| -> Option<f64> {
        let v = tokens.get(*i)?.parse::<f64>().ok()?;
        *i += 1;
        Some(v)
    };
    while i < tokens.len() {
        let t = tokens[i];
        if let Some(c) = t.chars().next().filter(|c| c.is_ascii_alphabetic()) {
            if t.len() != 1 {
                return None;
            }
            i += 1;
            match c {
                'Z' => cur.cmds.push(PathCmd::Close),
                'N' => {
                    if !cur.cmds.is_empty() {
                        paths.push(std::mem::replace(&mut cur, new()));
                    }
                }
                'F' => cur.fill = false,
                'S' => cur.stroke = false,
                'M' | 'L' | 'C' | 'Q' => cmd = c,
                _ => return None,
            }
            continue;
        }
        match cmd {
            'M' => {
                let (x, y) = (num(&mut i)?, num(&mut i)?);
                cur.cmds.push(PathCmd::Move(x, y));
                cmd = 'L';
            }
            'L' => {
                let (x, y) = (num(&mut i)?, num(&mut i)?);
                cur.cmds.push(PathCmd::Line(x, y));
            }
            'C' => {
                let v = [num(&mut i)?, num(&mut i)?, num(&mut i)?, num(&mut i)?, num(&mut i)?, num(&mut i)?];
                cur.cmds.push(PathCmd::Cubic(v[0], v[1], v[2], v[3], v[4], v[5]));
            }
            'Q' => {
                let v = [num(&mut i)?, num(&mut i)?, num(&mut i)?, num(&mut i)?];
                cur.cmds.push(PathCmd::Quad(v[0], v[1], v[2], v[3]));
            }
            _ => return None,
        }
    }
    if !cur.cmds.is_empty() {
        paths.push(cur);
    }
    (!paths.is_empty()).then_some(paths)
}

/// The paths as one `a:custGeom`, the way PowerPoint writes it.
pub fn to_cust_geom(paths: &[FreePath]) -> String {
    let n = |v: f64| v.round() as i64;
    let mut out = String::from("<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"l\" t=\"t\" r=\"r\" b=\"b\"/><a:pathLst>");
    for p in paths {
        out.push_str(&format!("<a:path w=\"{}\" h=\"{}\"", n(p.w), n(p.h)));
        if !p.fill {
            out.push_str(" fill=\"none\"");
        }
        if !p.stroke {
            out.push_str(" stroke=\"0\"");
        }
        out.push('>');
        let pt = |x: f64, y: f64| format!("<a:pt x=\"{}\" y=\"{}\"/>", n(x), n(y));
        for c in &p.cmds {
            out.push_str(&match *c {
                PathCmd::Move(x, y) => format!("<a:moveTo>{}</a:moveTo>", pt(x, y)),
                PathCmd::Line(x, y) => format!("<a:lnTo>{}</a:lnTo>", pt(x, y)),
                PathCmd::Cubic(a, b, c2, d, x, y) => format!("<a:cubicBezTo>{}{}{}</a:cubicBezTo>", pt(a, b), pt(c2, d), pt(x, y)),
                PathCmd::Quad(a, b, x, y) => format!("<a:quadBezTo>{}{}</a:quadBezTo>", pt(a, b), pt(x, y)),
                PathCmd::Arc { wr, hr, start, swing } => format!(
                    "<a:arcTo wR=\"{}\" hR=\"{}\" stAng=\"{}\" swAng=\"{}\"/>",
                    n(wr),
                    n(hr),
                    n(start * 60_000.0),
                    n(swing * 60_000.0)
                ),
                PathCmd::Close => "<a:close/>".to_string(),
            });
        }
        out.push_str("</a:path>");
    }
    out.push_str("</a:pathLst></a:custGeom>");
    out
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A quarter-circle wedge, and an open curve neither filled nor closed.
    pub(in crate::engine) fn wedge() -> Vec<FreePath> {
        vec![
            FreePath {
                w: 1000.0,
                h: 1000.0,
                cmds: vec![PathCmd::Move(500.0, 500.0), PathCmd::Line(1000.0, 500.0), PathCmd::Arc { wr: 500.0, hr: 500.0, start: 0.0, swing: 90.0 }, PathCmd::Close],
                fill: true,
                stroke: true,
            },
            FreePath { w: 1000.0, h: 1000.0, cmds: vec![PathCmd::Move(0.0, 0.0), PathCmd::Cubic(100.0, 0.0, 200.0, 50.0, 300.0, 100.0)], fill: false, stroke: true },
        ]
    }

    /// A quarter arc of a circle ends where the circle says, as Béziers.
    #[test]
    fn an_arc_ends_on_its_ellipse() {
        let cubics = arc_as_cubics((1000.0, 500.0), 500.0, 500.0, 0.0, 90.0);
        assert_eq!(cubics.len(), 1);
        let end = cubics[0][2];
        assert!((end.0 - 500.0).abs() < 1e-9 && (end.1 - 1000.0).abs() < 1e-9, "{end:?}");
        assert_eq!(arc_as_cubics((0.0, 0.0), 1.0, 1.0, 0.0, 270.0).len(), 3);
    }

    /// Through ODF's enhanced path and back, a path keeps its commands
    /// (an arc as Béziers), its fill and its outline.
    #[test]
    fn paths_survive_an_enhanced_path() {
        let paths = wedge();
        let d = to_enhanced_path(&paths, 1000.0);
        let back = from_enhanced_path(&d, 1000.0, 1000.0).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].cmds[..2], paths[0].cmds[..2]);
        assert!(matches!(back[0].cmds[2], PathCmd::Cubic(_, _, _, _, x, y) if (x - 500.0).abs() < 1.0 && (y - 1000.0).abs() < 1.0));
        assert_eq!((back[1].fill, back[1].stroke), (false, true));
        assert_eq!(from_enhanced_path("M 0 0 U 1 2 3 4 5 6 N", 10.0, 10.0), None, "an arc kind we do not draw");
    }
}
