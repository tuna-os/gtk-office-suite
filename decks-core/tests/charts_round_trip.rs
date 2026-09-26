// charts_round_trip.rs — a chart of every kind comes back from pptx and
// odp as it was saved: its kind, series, points and box, in slide order
// among the other objects, and a second save changes nothing. The package
// carries each chart as its format's own chart part. soffice_oracle.rs
// checks what LibreOffice makes of them.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Read;

use decks_core::engine::chart::{ChartData, ChartKind};
use decks_core::engine::{Deck, SlideObject};

fn chart_at(kind: ChartKind, x: f64, y: f64) -> SlideObject {
    let mut chart = ChartData::sample(kind);
    if kind == ChartKind::Line {
        // A category and a series name that need escaping, and a
        // negative and a fractional value.
        chart.series = "Revenue & <costs>".into();
        chart.points[1] = ("Q2 \"late\"".into(), -1.25);
    }
    SlideObject::Chart { x, y, w: 400.0, h: 250.0, rotation: 0.0, chart }
}

/// Two slides: a bar and a pie chart around a text box, then a line, an
/// area and a scatter chart.
pub fn deck() -> Deck {
    let mut d = Deck::new();
    let proto = d.slides[0].clone();
    let mut a = proto.clone();
    a.objects = vec![
        chart_at(ChartKind::Bar, 40.0, 60.0),
        SlideObject::TextBox {
            text: "Results".into(),
            x: 80.0,
            y: 10.0,
            w: 400.0,
            h: 40.0,
            rotation: 0.0,
            runs: vec![],
            body: Default::default(),
        },
        chart_at(ChartKind::Pie, 500.0, 60.0),
    ];
    let mut b = proto;
    b.objects = vec![
        chart_at(ChartKind::Line, 20.0, 20.0),
        chart_at(ChartKind::Area, 500.0, 20.0),
        chart_at(ChartKind::Scatter, 260.0, 280.0),
    ];
    d.slides = vec![a, b];
    d
}

fn r(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Each slide's objects: a chart as its box and data, anything else as
/// its kind.
fn objects(d: &Deck) -> Vec<Vec<String>> {
    d.slides
        .iter()
        .map(|s| {
            s.objects
                .iter()
                .map(|o| match o {
                    SlideObject::Chart { x, y, w, h, chart, .. } => {
                        format!("{} {} {} {} {:?}", r(*x), r(*y), r(*w), r(*h), chart)
                    }
                    SlideObject::TextBox { text, .. } => format!("text {text:?}"),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

fn read_back(d: &Deck, ext: &str, dir: &std::path::Path, stem: &str) -> Deck {
    let path = dir.join(format!("{stem}.{ext}"));
    decks_core::write_deck(path.to_str().unwrap(), d).unwrap();
    decks_core::read_deck(path.to_str().unwrap()).unwrap()
}

#[test]
fn every_kind_of_chart_survives_pptx_and_odp() {
    let d = deck();
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let back = read_back(&d, ext, dir.path(), "charts");
        assert_eq!(objects(&back), objects(&d), "{ext}");
        let twice = read_back(&back, ext, dir.path(), "again");
        assert_eq!(objects(&twice), objects(&d), "{ext}: a second save");
    }
}

fn part(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> String {
    let mut s = String::new();
    zip.by_name(name).unwrap_or_else(|_| panic!("{name} is in the package")).read_to_string(&mut s).unwrap();
    s
}

/// A pptx chart is a chart part, typed in [Content_Types].xml and named
/// by a chart relationship of its slide.
#[test]
fn a_pptx_chart_is_a_typed_chart_part_its_slide_relates_to() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("charts.pptx");
    decks_core::write_deck(path.to_str().unwrap(), &deck()).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let types = part(&mut zip, "[Content_Types].xml");
    for n in 1..=5 {
        assert!(
            types.contains(&format!(
                "PartName=\"/ppt/charts/chart{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.chart+xml\""
            )),
            "chart{n} typed: {types}"
        );
    }
    assert!(part(&mut zip, "ppt/charts/chart1.xml").contains("<c:barChart>"));
    assert!(part(&mut zip, "ppt/charts/chart2.xml").contains("<c:pieChart>"));
    assert!(part(&mut zip, "ppt/charts/chart5.xml").contains("<c:scatterChart>"));
    let rels = part(&mut zip, "ppt/slides/_rels/slide2.xml.rels");
    assert!(
        rels.contains("relationships/chart\" Target=\"../charts/chart3.xml\""),
        "slide 2's first chart is the deck's third: {rels}"
    );
    let slide = part(&mut zip, "ppt/slides/slide1.xml");
    assert!(slide.contains("<a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">"), "{slide}");
}

/// An odp chart is an embedded chart object the manifest names.
#[test]
fn an_odp_chart_is_an_embedded_object_the_manifest_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("charts.odp");
    decks_core::write_deck(path.to_str().unwrap(), &deck()).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let manifest = part(&mut zip, "META-INF/manifest.xml");
    for n in 1..=5 {
        assert!(
            manifest.contains(&format!(
                "manifest:full-path=\"Object {n}/\" manifest:media-type=\"application/vnd.oasis.opendocument.chart\""
            )),
            "Object {n}: {manifest}"
        );
        assert!(manifest.contains(&format!("manifest:full-path=\"Object {n}/content.xml\"")), "{manifest}");
    }
    assert!(part(&mut zip, "Object 2/content.xml").contains("chart:class=\"chart:circle\""));
    let content = part(&mut zip, "content.xml");
    assert!(content.contains("<draw:object xlink:href=\"./Object 1\""), "{content}");
}

/// A picture Impress writes beside a chart object, its stand-in for
/// readers that can't draw the chart, is not read as a second object.
#[test]
fn an_objects_replacement_picture_is_not_a_second_object() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("charts.odp");
    let mut d = Deck::new();
    d.slides[0].objects = vec![chart_at(ChartKind::Bar, 10.0, 10.0)];
    decks_core::write_deck(path.to_str().unwrap(), &d).unwrap();
    // Rewrite the package with the frame holding a replacement too.
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let names: Vec<String> = zip.file_names().map(str::to_string).collect();
    let out_path = dir.path().join("with-replacement.odp");
    let mut out = zip::ZipWriter::new(std::fs::File::create(&out_path).unwrap());
    let opt = zip::write::SimpleFileOptions::default();
    for name in names {
        if name.ends_with('/') {
            out.add_directory(name.as_str(), opt).unwrap();
            continue;
        }
        let mut bytes = Vec::new();
        zip.by_name(&name).unwrap().read_to_end(&mut bytes).unwrap();
        if name == "content.xml" {
            let s = String::from_utf8(bytes).unwrap().replace(
                "xlink:actuate=\"onLoad\"/></draw:frame>",
                "xlink:actuate=\"onLoad\"/><draw:image xlink:href=\"./ObjectReplacements/Object 1\"/></draw:frame>",
            );
            bytes = s.into_bytes();
        }
        use std::io::Write;
        out.start_file(name.as_str(), opt).unwrap();
        out.write_all(&bytes).unwrap();
    }
    use std::io::Write;
    out.start_file("ObjectReplacements/Object 1", opt).unwrap();
    out.write_all(b"not really a picture").unwrap();
    out.finish().unwrap();
    let back = decks_core::read_deck(out_path.to_str().unwrap()).unwrap();
    assert_eq!(objects(&back), objects(&d));
}
