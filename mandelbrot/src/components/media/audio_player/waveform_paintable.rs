use std::borrow::Cow;

use gtk::{gdk, glib, graphene, prelude::*, subclass::prelude::*};

use super::waveform::{WAVEFORM_HEIGHT, WAVEFORM_HEIGHT_I32};
use crate::utils::resample_slice;

/// The width of the bars in the waveform.
const BAR_WIDTH: f32 = 2.0;
/// The horizontal padding around bars in the waveform.
const BAR_HORIZONTAL_PADDING: f32 = 1.0;
/// The full width of a bar, including its padding.
const BAR_FULL_WIDTH: f32 = BAR_WIDTH + 2.0 * BAR_HORIZONTAL_PADDING;
/// The minimum height of the bars in the waveform.
///
/// We do not want to have holes in the waveform so we restrict the minimum
/// height.
const BAR_MIN_HEIGHT: f32 = 2.0;
/// The waveform used as fallback.
///
/// It will generate a full waveform.
const WAVEFORM_FALLBACK: &[f32] = &[1.0];

mod imp {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::WaveformPaintable)]
    pub struct WaveformPaintable {
        /// The waveform to display.
        ///
        /// The values must be normalized between 0 and 1.
        waveform: RefCell<Cow<'static, [f32]>>,
        /// The previous waveform that was displayed, if any.
        ///
        /// Use for the transition between waveforms.
        previous_waveform: RefCell<Option<Cow<'static, [f32]>>>,
        /// The progress of the transition between waveforms.
        #[property(get, set = Self::set_transition_progress, explicit_notify)]
        transition_progress: Cell<f64>,
    }

    impl Default for WaveformPaintable {
        fn default() -> Self {
            Self {
                waveform: RefCell::new(Cow::Borrowed(WAVEFORM_FALLBACK)),
                previous_waveform: Default::default(),
                transition_progress: Cell::new(1.0),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WaveformPaintable {
        const NAME: &'static str = "WaveformPaintable";
        type Type = super::WaveformPaintable;
        type Interfaces = (gdk::Paintable,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for WaveformPaintable {}

    impl PaintableImpl for WaveformPaintable {
        fn intrinsic_width(&self) -> i32 {
            (self.waveform.borrow().len() as f32 * BAR_FULL_WIDTH) as i32
        }

        fn intrinsic_height(&self) -> i32 {
            WAVEFORM_HEIGHT_I32
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, _height: f64) {
            if width <= 0.0 {
                return;
            }

            let exact_samples_needed = width as f32 / BAR_FULL_WIDTH;

            // If the number of samples has a fractional part, compute a padding to center
            // the waveform horizontally in the paintable.
            let waveform_start_padding = (exact_samples_needed.fract() * BAR_FULL_WIDTH).trunc();
            // We are sure that the number of samples is positive.
            #[allow(clippy::cast_sign_loss)]
            let samples_needed = exact_samples_needed.trunc() as usize;

            let mut waveform =
                resample_slice(self.waveform.borrow().as_ref(), samples_needed).into_owned();

            // If there is a previous waveform, we have an ongoing transition.
            if let Some(previous_waveform) = self.previous_waveform.borrow().as_ref()
                && *previous_waveform != waveform
            {
                let previous_waveform = resample_slice(previous_waveform, samples_needed);
                let progress = self.transition_progress.get() as f32;

                // Compute the current waveform for the ongoing transition.
                waveform = waveform
                    .into_iter()
                    .zip(previous_waveform.iter())
                    .map(|(current, &previous)| {
                        (((current - previous) * progress) + previous).clamp(0.0, 1.0)
                    })
                    .collect();
            }

            for (pos, value) in waveform.into_iter().enumerate() {
                let x = waveform_start_padding + pos as f32 * (BAR_FULL_WIDTH);
                let height = (WAVEFORM_HEIGHT * value).max(BAR_MIN_HEIGHT);
                // Center the bar vertically.
                let y = (WAVEFORM_HEIGHT - height) / 2.0;

                let rect = graphene::Rect::new(x, y, BAR_WIDTH, height);
                snapshot.append_color(&gdk::RGBA::WHITE, &rect);
            }
        }
    }

    impl WaveformPaintable {
        /// Set the values of the bars to display.
        ///
        /// The values must be normalized between 0 and 1.
        ///
        /// Returns whether the waveform changed.
        pub(super) fn set_waveform(&self, waveform: Vec<f32>) -> bool {
            let waveform = if waveform.is_empty() {
                Cow::Borrowed(WAVEFORM_FALLBACK)
            } else {
                Cow::Owned(waveform)
            };

            if *self.waveform.borrow() == waveform {
                return false;
            }

            let previous = self.waveform.replace(waveform);
            self.previous_waveform.replace(Some(previous));

            self.obj().invalidate_contents();

            true
        }

        /// Set the progress of the transition between waveforms.
        fn set_transition_progress(&self, progress: f64) {
            if (self.transition_progress.get() - progress).abs() < 0.000_001 {
                return;
            }

            self.transition_progress.set(progress);

            if (progress - 1.0).abs() < 0.000_001 {
                // This is the end of the transition, we can drop the previous waveform.
                self.previous_waveform.take();
            }

            let obj = self.obj();
            obj.notify_transition_progress();
            obj.invalidate_contents();
        }
    }
}

glib::wrapper! {
    /// A paintable displaying a waveform.
    pub struct WaveformPaintable(ObjectSubclass<imp::WaveformPaintable>)
        @implements gdk::Paintable;
}

impl WaveformPaintable {
    /// Create a new empty `WaveformPaintable`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the waveform to display.
    ///
    /// The values must be normalized between 0 and 1.
    ///
    /// Returns whether the waveform changed.
    pub(crate) fn set_waveform(&self, waveform: Vec<f32>) -> bool {
        self.imp().set_waveform(waveform)
    }
}

impl Default for WaveformPaintable {
    fn default() -> Self {
        Self::new()
    }
}
