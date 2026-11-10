//! The attachment lightbox: images and PDFs shown over a whole window, from
//! the attachment drawer or a card's attachment row.
//!
//! A component of its own so that a message's own window can have one as
//! well as the main window. Each lays the lightbox over its content with a
//! `gtk::Overlay`; it looks like the gallery's own lightbox (same CSS).

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use crate::relm::prelude::*;

use crate::i18n::{i18n, i18n_f};
use crate::models::Attachment;
use crate::ui::thumbnails;

pub(crate) struct Lightbox {
    items: Vec<Attachment>,
    pos: usize,
    texture: Option<gtk::gdk::Texture>,
    /// The current item has nothing to show: a PDF poppler cannot open, or
    /// an image that does not decode.
    no_preview: bool,
    /// Current zoom (1 or 3): a click on the document toggles it, Escape
    /// unwinds it before closing.
    zoom: i32,
    picture: gtk::Picture,
    scroller: gtk::ScrolledWindow,
    /// The lightbox itself: zoomed, the scroller takes all of it.
    root: gtk::Box,
    /// Whether the window's key handler has been added: once, the first
    /// time the lightbox opens, when the window it sits in is known.
    keys_added: bool,
    /// Mirror of "the lightbox is open", read by that key handler.
    open: Rc<Cell<bool>>,
}

#[derive(Debug)]
pub(crate) enum LightboxInput {
    /// Show these previewable attachments, starting at `start`.
    Show {
        items: Vec<Attachment>,
        start: usize,
    },
    Prev,
    Next,
    Close,
    /// Click on the document: toggle zoom 1x ↔ 3x, anchored at the clicked
    /// point (picture coordinates at the fitted size).
    ZoomCycle {
        x: f64,
        y: f64,
    },
    /// Escape: unwind zoom first; close only from normal view.
    Escape,
    /// Open the shown item in its default application.
    OpenCurrent,
    /// Save the shown item via a file chooser.
    DownloadCurrent,
    /// A full-size PDF render finished (content hash, and the page unless
    /// it failed): show it if that item is still on screen.
    Rendered(u64, Option<gtk::gdk::Texture>),
}

/// The widgets the lightbox refreshes from its model after every message.
///
/// This is what `#[relm4::component]`'s `view!` block used to generate: the
/// widget tree, the handles to the parts a `#[watch]` attribute drove, and
/// `update_view` setting them again. It is written out so the component no
/// longer needs the macro.
pub(crate) struct LightboxWidgets {
    root: gtk::Box,
    top_bar: gtk::CenterBox,
    title: gtk::Label,
    middle: gtk::Box,
    prev: gtk::Button,
    next: gtk::Button,
    stack: gtk::Stack,
    scroller: gtk::ScrolledWindow,
    picture: gtk::Picture,
    file_icon: gtk::Image,
    bottom_bar: gtk::CenterBox,
    caption: gtk::Label,
}

impl LightboxWidgets {
    /// Set every property that depends on the model (relm4's `#[watch]`).
    fn apply(&self, model: &Lightbox) {
        // Open while there is something to show. Zoomed, the bars, arrows
        // and padding step aside and the document fills the window.
        self.root.set_visible(!model.items.is_empty());
        if model.zoom != 1 {
            self.root.add_css_class("zoomed");
        } else {
            self.root.remove_css_class("zoomed");
        }
        let normal = model.zoom == 1;
        let current = model.items.get(model.pos);

        self.top_bar.set_visible(normal);
        self.title.set_label(current.map(|a| a.name.as_str()).unwrap_or(""));

        self.middle.set_spacing(if normal { 8 } else { 0 });
        let several = model.items.len() > 1 && normal;
        self.prev.set_visible(several);
        self.next.set_visible(several);

        self.stack.set_visible_child_name(if model.texture.is_some() {
            "image"
        } else if model.no_preview {
            "no-preview"
        } else {
            "rendering"
        });
        self.picture.set_paintable(model.texture.as_ref());
        self.file_icon.set_icon_name(current.map(|a| thumbnails::icon_for(&a.name)));
        self.file_icon.set_css_classes(&[
            "gallery-file-icon",
            current.map(|a| thumbnails::icon_color_class(&a.name)).unwrap_or("ftype-generic"),
        ]);

        self.bottom_bar.set_visible(normal);
        self.caption.set_label(&model.caption());
    }
}

/// A button that sends `message()` to the component when clicked.
fn on_click(button: &gtk::Button, sender: &ComponentSender<Lightbox>, message: fn() -> LightboxInput) {
    let input = sender.input_sender().clone();
    button.connect_clicked(move |_| input.emit(message()));
}

/// A button with an icon, a tooltip and style classes.
fn icon_button(icon: &str, tooltip: &str, classes: &[&str]) -> gtk::Button {
    let button = gtk::Button::builder().icon_name(icon).tooltip_text(tooltip).build();
    for class in classes {
        button.add_css_class(class);
    }
    button
}

/// Build the lightbox's widget tree into `root`.
fn build_widgets(root: &gtk::Box, sender: &ComponentSender<Lightbox>, model: &Lightbox) -> LightboxWidgets {
    // Top bar: the item's name, and a close button.
    let top_bar = gtk::CenterBox::new();
    top_bar.add_css_class("gallery-lightbox-bar");
    let title = gtk::Label::new(None);
    title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    title.set_halign(gtk::Align::Start);
    title.add_css_class("gallery-lightbox-title");
    top_bar.set_start_widget(Some(&title));
    let close = icon_button("window-close-symbolic", &i18n("Close"), &["circular", "flat"]);
    on_click(&close, sender, || LightboxInput::Close);
    top_bar.set_end_widget(Some(&close));
    root.append(&top_bar);

    // Middle: previous, the document, next.
    let middle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    middle.set_vexpand(true);

    let prev = icon_button("go-previous-symbolic", &i18n("Previous"), &["circular", "osd"]);
    prev.set_valign(gtk::Align::Center);
    on_click(&prev, sender, || LightboxInput::Prev);
    middle.append(&prev);

    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let picture = gtk::Picture::new();
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk::ContentFit::Contain);
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Automatic);
    scroller.set_vscrollbar_policy(gtk::PolicyType::Automatic);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&picture));
    stack.add_named(&scroller, Some("image"));

    let rendering = gtk::Box::new(gtk::Orientation::Vertical, 0);
    rendering.set_halign(gtk::Align::Center);
    rendering.set_valign(gtk::Align::Center);
    let spinner = gtk::Spinner::new();
    spinner.set_spinning(true);
    spinner.set_width_request(36);
    spinner.set_height_request(36);
    rendering.append(&spinner);
    stack.add_named(&rendering, Some("rendering"));

    let no_preview = gtk::Box::new(gtk::Orientation::Vertical, 12);
    no_preview.add_css_class("gallery-lightbox-file");
    no_preview.set_halign(gtk::Align::Center);
    no_preview.set_valign(gtk::Align::Center);
    let file_icon = gtk::Image::new();
    file_icon.set_pixel_size(96);
    no_preview.append(&file_icon);
    let nothing = gtk::Label::new(Some(i18n("No preview available").as_str()));
    nothing.add_css_class("title-3");
    no_preview.append(&nothing);
    stack.add_named(&no_preview, Some("no-preview"));
    middle.append(&stack);

    let next = icon_button("go-next-symbolic", &i18n("Next"), &["circular", "osd"]);
    next.set_valign(gtk::Align::Center);
    on_click(&next, sender, || LightboxInput::Next);
    middle.append(&next);
    root.append(&middle);

    // Bottom bar: the caption, and open and download.
    let bottom_bar = gtk::CenterBox::new();
    bottom_bar.add_css_class("gallery-lightbox-bar");
    let caption = gtk::Label::new(None);
    caption.set_halign(gtk::Align::Start);
    caption.set_ellipsize(gtk::pango::EllipsizeMode::End);
    caption.add_css_class("dim-label");
    bottom_bar.set_start_widget(Some(&caption));
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let open = icon_button("document-open-symbolic", &i18n("Open"), &["flat"]);
    on_click(&open, sender, || LightboxInput::OpenCurrent);
    actions.append(&open);
    let download = icon_button("folder-download-symbolic", &i18n("Download…"), &["flat"]);
    on_click(&download, sender, || LightboxInput::DownloadCurrent);
    actions.append(&download);
    bottom_bar.set_end_widget(Some(&actions));
    root.append(&bottom_bar);

    let widgets = LightboxWidgets {
        root: root.clone(),
        top_bar,
        title,
        middle,
        prev,
        next,
        stack,
        scroller,
        picture,
        file_icon,
        bottom_bar,
        caption,
    };
    widgets.apply(model);
    widgets
}

impl Component for Lightbox {
    type Init = ();
    type Input = LightboxInput;
    type Output = ();
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = LightboxWidgets;

    fn init_root() -> gtk::Box {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("gallery-lightbox");
        root
    }

    fn update_view(&self, widgets: &mut LightboxWidgets, _sender: ComponentSender<Self>) {
        widgets.apply(self);
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let mut model = Lightbox {
            items: Vec::new(),
            pos: 0,
            texture: None,
            no_preview: false,
            zoom: 1,
            picture: gtk::Picture::new(),
            scroller: gtk::ScrolledWindow::new(),
            root: root.clone(),
            keys_added: false,
            open: Rc::new(Cell::new(false)),
        };
        let widgets = build_widgets(&root, &sender, &model);
        model.picture = widgets.picture.clone();
        model.scroller = widgets.scroller.clone();

        // Pointer behaviour: dragging pans the zoomed document (the
        // scroller's adjustments move opposite the pointer); a clean click,
        // a release with no meaningful movement, cycles the zoom. The shared
        // `moved` cell is what keeps a pan from also zooming.
        {
            let hadj = model.scroller.hadjustment();
            let vadj = model.scroller.vadjustment();
            let start = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
            let moved = Rc::new(Cell::new(0.0_f64));

            let drag = gtk::GestureDrag::new();
            drag.set_button(gtk::gdk::BUTTON_PRIMARY);
            {
                let start = start.clone();
                let moved = moved.clone();
                let (h, v) = (hadj.clone(), vadj.clone());
                drag.connect_drag_begin(move |_, _, _| {
                    start.set((h.value(), v.value()));
                    moved.set(0.0);
                });
            }
            {
                let start = start.clone();
                let moved = moved.clone();
                drag.connect_drag_update(move |_, dx, dy| {
                    moved.set(moved.get().max(dx.abs().max(dy.abs())));
                    let (h0, v0) = start.get();
                    hadj.set_value(h0 - dx);
                    vadj.set_value(v0 - dy);
                });
            }
            // On the SCROLLER, not the picture: the picture's own coordinate
            // space moves with every pan, so offsets measured in it oscillate
            // (scroll, shift, un-scroll) and the drag jitters. The scroller
            // stays put, so its offsets are stable.
            model.scroller.add_controller(drag);

            let click = gtk::GestureClick::new();
            click.set_button(gtk::gdk::BUTTON_PRIMARY);
            let s = sender.clone();
            click.connect_released(move |_, n, x, y| {
                if n == 1 && moved.get() < 8.0 {
                    s.input_sender().emit(LightboxInput::ZoomCycle { x, y });
                }
            });
            model.picture.add_controller(click);
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: LightboxInput, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            LightboxInput::Show { items, start } => {
                if items.is_empty() {
                    return;
                }
                self.add_keys(root, &sender);
                self.pos = start.min(items.len() - 1);
                self.items = items;
                self.open.set(true);
                self.set_zoom(1);
                self.refresh(&sender);
            }
            LightboxInput::Prev => self.step(-1, &sender),
            LightboxInput::Next => self.step(1, &sender),
            LightboxInput::Close => {
                self.items.clear();
                self.show_page(None);
                self.open.set(false);
                self.set_zoom(1);
            }
            LightboxInput::ZoomCycle { x, y } => {
                if self.zoom == 1 {
                    self.zoom_to_point(x, y);
                } else {
                    self.set_zoom(1);
                }
            }
            LightboxInput::Escape => {
                // Zoomed in, Escape returns to the fitted view; from there it
                // closes the lightbox.
                if self.zoom != 1 {
                    self.set_zoom(1);
                } else {
                    sender.input_sender().emit(LightboxInput::Close);
                }
            }
            LightboxInput::OpenCurrent => {
                if let Some(att) = self.items.get(self.pos) {
                    let window = root.root().and_downcast::<gtk::Window>();
                    crate::ui::attachment_io::open_bytes(&att.name, &att.data, window.as_ref());
                }
            }
            LightboxInput::DownloadCurrent => {
                if let Some(att) = self.items.get(self.pos).cloned() {
                    let window = root.root().and_downcast::<gtk::Window>();
                    // A failed write is dropped.
                    crate::ui::attachment_io::save_attachment_as(
                        &att.name,
                        att.data,
                        window.as_ref(),
                        |_| {},
                    );
                }
            }
            LightboxInput::Rendered(key, page) => {
                let still_current = self
                    .items
                    .get(self.pos)
                    .is_some_and(|a| thumbnails::content_key(&a.data) == key);
                if still_current {
                    self.show_page(page);
                }
            }
        }
    }
}

impl Lightbox {
    /// Escape and the arrow keys drive the lightbox from anywhere in its
    /// window: capture phase, and only while it is open, so typing
    /// elsewhere is untouched.
    fn add_keys(&mut self, root: &gtk::Box, sender: &ComponentSender<Self>) {
        if self.keys_added {
            return;
        }
        let Some(window) = root.root().and_downcast::<gtk::Window>() else {
            return;
        };
        self.keys_added = true;
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let open = self.open.clone();
        let s = sender.input_sender().clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if !open.get() {
                return gtk::glib::Propagation::Proceed;
            }
            let msg = match key {
                gtk::gdk::Key::Escape => LightboxInput::Escape,
                gtk::gdk::Key::Left => LightboxInput::Prev,
                gtk::gdk::Key::Right => LightboxInput::Next,
                _ => return gtk::glib::Propagation::Proceed,
            };
            let _ = s.send(msg);
            gtk::glib::Propagation::Stop
        });
        window.add_controller(keys);
    }

    /// "name · n of m" for the bottom bar.
    fn caption(&self) -> String {
        match self.items.get(self.pos) {
            Some(att) => i18n_f(
                "{name} · {current} of {total}",
                &[
                    ("name", &att.name),
                    ("current", &(self.pos + 1).to_string()),
                    ("total", &self.items.len().to_string()),
                ],
            ),
            None => String::new(),
        }
    }

    fn step(&mut self, delta: i32, sender: &ComponentSender<Self>) {
        let n = self.items.len() as i32;
        if n == 0 {
            return;
        }
        self.pos = (((self.pos as i32 + delta) % n + n) % n) as usize;
        self.set_zoom(1);
        self.refresh(sender);
    }

    /// Zoom to 3x anchored at `(x, y)`, the clicked point in the fitted
    /// picture's coordinates. The zoomed scroller is the whole lightbox,
    /// not the fitted one between the bars, so the scale is not exactly 3:
    /// the click is turned into a point on the document first, and once the
    /// resize has been laid out (the scroller's range exists) the
    /// adjustments put that point at the viewport's centre. Without this the
    /// view stayed at the top-left of the grown, mostly-letterboxed box, the
    /// content apparently shoved off-screen.
    fn zoom_to_point(&mut self, x: f64, y: f64) {
        let Some((tw, th)) = self
            .texture
            .as_ref()
            .map(|t| (f64::from(t.width()), f64::from(t.height())))
        else {
            return;
        };
        if tw <= 0.0 || th <= 0.0 {
            return;
        }
        // Where the click lands on the document, 0..1 each way.
        let fit = move |w: f64, h: f64| {
            let s = (w / tw).min(h / th);
            (s, (w - tw * s) / 2.0, (h - th * s) / 2.0)
        };
        let (s0, ox0, oy0) = fit(
            f64::from(self.picture.width()),
            f64::from(self.picture.height()),
        );
        let u = ((x - ox0) / (tw * s0)).clamp(0.0, 1.0);
        let v = ((y - oy0) / (th * s0)).clamp(0.0, 1.0);

        self.set_zoom(3);
        let hadj = self.scroller.hadjustment();
        let vadj = self.scroller.vadjustment();
        let tries = Cell::new(0u8);
        self.picture.add_tick_callback(move |_, _| {
            let laid_out =
                hadj.upper() > hadj.page_size() + 1.0 || vadj.upper() > vadj.page_size() + 1.0;
            if laid_out {
                let (s1, ox1, oy1) = fit(hadj.upper(), vadj.upper());
                hadj.set_value(ox1 + u * tw * s1 - hadj.page_size() / 2.0);
                vadj.set_value(oy1 + v * th * s1 - vadj.page_size() / 2.0);
                return gtk::glib::ControlFlow::Break;
            }
            tries.set(tries.get() + 1);
            if tries.get() > 30 {
                return gtk::glib::ControlFlow::Break;
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    /// At 1x the picture fits its scroller; at 3x its box grows to that
    /// multiple of the whole lightbox, which the scroller fills once the
    /// bars have gone (Contain keeps the aspect), and the scroller pans the
    /// overflow.
    fn set_zoom(&mut self, zoom: i32) {
        self.zoom = zoom;
        if zoom <= 1 {
            self.picture.set_size_request(-1, -1);
        } else {
            self.picture
                .set_size_request(self.root.width() * zoom, self.root.height() * zoom);
        }
    }

    /// The outcome for the current item; no page at all means it has none
    /// to show, which is not the same as one still on its way.
    fn show_page(&mut self, page: Option<gtk::gdk::Texture>) {
        self.no_preview = page.is_none();
        self.texture = page;
    }

    /// Work out the texture for the current item: images decode on the
    /// spot; a PDF's page comes from the shared full-size cache or a worker
    /// render that circles back via [`LightboxInput::Rendered`].
    fn refresh(&mut self, sender: &ComponentSender<Self>) {
        self.texture = None;
        self.no_preview = false;
        let Some(att) = self.items.get(self.pos) else {
            return;
        };
        if crate::models::is_image_name(&att.name) {
            let image = thumbnails::texture_from(&att.data);
            self.show_page(image);
            return;
        }
        // The result travels in the message and does not come back through
        // here: a render that answered this function again looped for ever on
        // a PDF that fails, with the window frozen (#582).
        if let Some(page) = thumbnails::cached_pdf_preview(&att.data) {
            self.show_page(page);
            return;
        }
        let key = thumbnails::content_key(&att.data);
        let s = sender.input_sender().clone();
        thumbnails::lightbox_pdf_texture(&att.data, move |page| {
            let _ = s.send(LightboxInput::Rendered(key, page));
        });
    }
}
