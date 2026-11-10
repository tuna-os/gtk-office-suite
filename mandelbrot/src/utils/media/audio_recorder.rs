//! Audio recorder for voice messages.

use std::{
    future::IntoFuture,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures_util::future::{self, Either};
use gst::prelude::*;
use gtk::glib;
use tracing::warn;

use super::audio::normalize_waveform;
use crate::utils::{OneshotNotifier, OneshotNotifierReceiver};

/// The maximum duration of a voice message recording, ~30 minutes.
pub(crate) const MAX_RECORDING_DURATION: Duration = Duration::from_mins(30);

/// The interval between two peak level measurements.
const LEVEL_INTERVAL: Duration = Duration::from_millis(100);

/// The maximum time to wait for the pipeline to flush the remaining data when
/// stopping a recording.
const STOP_TIMEOUT: u32 = 5;

/// An error encountered while recording audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioRecorderError {
    /// The recording pipeline could not be constructed or started.
    ///
    /// This usually means that no microphone is accessible.
    Start,
    /// An error occurred while recording.
    Recording,
    /// The recording did not produce any data.
    NoData,
}

/// The shared state of the recording pipeline.
#[derive(Debug, Default)]
struct RecorderState {
    /// The encoded Ogg Opus data.
    data: Vec<u8>,
    /// The peak levels collected so far, as linear amplitudes.
    peaks: Vec<f64>,
    /// Whether an error occurred in the pipeline.
    has_error: bool,
}

/// A recorded voice message.
#[derive(Debug, Clone)]
pub(crate) struct RecordedAudio {
    /// The encoded Ogg Opus data.
    pub(crate) data: Vec<u8>,
    /// The duration of the recording.
    pub(crate) duration: Duration,
    /// The normalized waveform of the recording, with values between 0 and 1.
    pub(crate) waveform: Vec<f32>,
}

/// A recorder to capture audio from the default microphone as Ogg Opus.
pub(crate) struct AudioRecorder {
    pipeline: gst::Pipeline,
    state: Arc<Mutex<RecorderState>>,
    /// The receiver resolving when the pipeline terminates, because of EOS or
    /// an error.
    ended_receiver: OneshotNotifierReceiver<()>,
    _bus_guard: gst::bus::BusWatchGuard,
    started_at: Instant,
}

impl std::fmt::Debug for AudioRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioRecorder")
            .field("started_at", &self.started_at)
            .finish_non_exhaustive()
    }
}

impl AudioRecorder {
    /// Start recording audio from the default audio input.
    pub(crate) fn start() -> Result<Self, AudioRecorderError> {
        let pipeline = match gst::parse::launch(&format!(
            "autoaudiosrc ! audioconvert ! audioresample ! audio/x-raw,channels=1 ! level name=level interval={} ! opusenc ! oggmux ! appsink name=sink",
            LEVEL_INTERVAL.as_nanos()
        )) {
            Ok(pipeline) => pipeline
                .downcast::<gst::Pipeline>()
                .expect("GstElement should be a GstPipeline"),
            Err(error) => {
                warn!("Could not create GstPipeline for voice recording: {error}");
                return Err(AudioRecorderError::Start);
            }
        };

        let appsink = pipeline
            .by_name("sink")
            .expect("sink element should be in the pipeline")
            .downcast::<gst_app::AppSink>()
            .expect("sink element should be an appsink");
        appsink.set_property("sync", false);

        let state = Arc::new(Mutex::new(RecorderState::default()));

        // Collect the encoded data.
        let state_clone = state.clone();
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let Some(buffer) = sample.buffer() else {
                        return Ok(gst::FlowSuccess::Ok);
                    };
                    let Ok(map) = buffer.map_readable() else {
                        warn!("Could not map voice recording buffer");
                        return Err(gst::FlowError::Error);
                    };

                    match state_clone.lock() {
                        Ok(mut state) => state.data.extend_from_slice(map.as_slice()),
                        Err(error) => {
                            warn!("Failed to lock voice recording state mutex: {error}");
                            return Err(gst::FlowError::Error);
                        }
                    }

                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        let (ended_receiver, bus_guard) = Self::watch_bus(&pipeline, state.clone());

        if let Err(error) = pipeline.set_state(gst::State::Playing) {
            warn!("Could not start GstPipeline for voice recording: {error}");
            let _ = pipeline.set_state(gst::State::Null);
            if let Some(bus) = pipeline.bus() {
                bus.set_flushing(true);
            }

            return Err(AudioRecorderError::Start);
        }

        Ok(Self {
            pipeline,
            state,
            ended_receiver,
            _bus_guard: bus_guard,
            started_at: Instant::now(),
        })
    }

    /// Watch the bus of the given pipeline for peak levels and the end of the
    /// recording.
    ///
    /// Returns a receiver resolving when the pipeline terminates, and the
    /// guard of the bus watch.
    fn watch_bus(
        pipeline: &gst::Pipeline,
        state: Arc<Mutex<RecorderState>>,
    ) -> (OneshotNotifierReceiver<()>, gst::bus::BusWatchGuard) {
        let ended_notifier = OneshotNotifier::<()>::new("voice_recording");
        let ended_receiver = ended_notifier.listen();
        let bus = pipeline.bus().expect("GstPipeline should have a GstBus");

        let bus_guard = bus
            .add_watch(move |_, message| {
                match message.view() {
                    gst::MessageView::Eos(_) => {
                        ended_notifier.notify();
                        glib::ControlFlow::Break
                    }
                    gst::MessageView::Error(error) => {
                        warn!("Could not record voice message: {error}");
                        if let Ok(mut state) = state.lock() {
                            state.has_error = true;
                        }
                        ended_notifier.notify();
                        glib::ControlFlow::Break
                    }
                    gst::MessageView::Element(element) => {
                        if let Some(structure) = element.structure()
                            && structure.has_name("level")
                        {
                            let rms_array = structure
                                .get::<&glib::ValueArray>("rms")
                                .expect("rms value should be a GValueArray");
                            let rms = rms_array[0]
                                .get::<f64>()
                                .expect("GValueArray value should be a double");

                            match state.lock() {
                                Ok(mut state) => {
                                    let value_db = if rms.is_nan() { 0.0 } else { rms };
                                    // Convert the decibels to a relative amplitude, to get a
                                    // value between 0 and 1.
                                    let value = 10.0_f64.powf(value_db / 20.0);

                                    state.peaks.push(value);
                                }
                                Err(error) => {
                                    warn!("Failed to lock voice recording state mutex: {error}");
                                }
                            }
                        }
                        glib::ControlFlow::Continue
                    }
                    _ => glib::ControlFlow::Continue,
                }
            })
            .expect("Adding GstBus watch should succeed");

        (ended_receiver, bus_guard)
    }

    /// The elapsed time since the recording started.
    pub(crate) fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// The latest peak level of the recording, as a value between 0 and 1.
    pub(crate) fn last_peak(&self) -> f64 {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.peaks.last().copied())
            .unwrap_or_default()
            .clamp(0.0, 1.0)
    }

    /// Whether an error occurred in the recording pipeline.
    pub(crate) fn has_error(&self) -> bool {
        self.state.lock().is_ok_and(|state| state.has_error)
    }

    /// Stop the recording and get the recorded audio.
    pub(crate) async fn stop(self) -> Result<RecordedAudio, AudioRecorderError> {
        let duration = self.started_at.elapsed();

        // Send EOS to flush the remaining data through the muxer, and wait for
        // it to reach the sink, with a timeout in case the pipeline is stuck.
        self.pipeline.send_event(gst::event::Eos::new());

        let timeout = std::pin::pin!(glib::timeout_future_seconds(STOP_TIMEOUT));
        if let Either::Right(_) = future::select(self.ended_receiver.into_future(), timeout).await {
            warn!("Timed out while waiting for the end of the voice recording");
        }

        let _ = self.pipeline.set_state(gst::State::Null);
        if let Some(bus) = self.pipeline.bus() {
            bus.set_flushing(true);
        }

        let (data, peaks, has_error) = match self.state.lock() {
            Ok(mut state) => (
                std::mem::take(&mut state.data),
                std::mem::take(&mut state.peaks),
                state.has_error,
            ),
            Err(error) => {
                warn!("Failed to lock voice recording state mutex: {error}");
                return Err(AudioRecorderError::Recording);
            }
        };

        if has_error {
            return Err(AudioRecorderError::Recording);
        }
        if data.is_empty() {
            return Err(AudioRecorderError::NoData);
        }

        Ok(RecordedAudio {
            data,
            duration,
            waveform: normalize_waveform(peaks),
        })
    }

    /// Cancel the recording and discard the recorded audio.
    pub(crate) fn cancel(self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        if let Some(bus) = self.pipeline.bus() {
            bus.set_flushing(true);
        }
    }
}
