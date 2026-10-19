use atomic_float::AtomicF32;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{Color32, ProgressBar, RichText, Vec2},
    resizable_window::ResizableWindow,
    widgets, EguiState,
};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

const PEAK_METER_DECAY_MS: f64 = 150.0;

/// What the host set up, for the editor's diagnostics.
#[derive(Default)]
struct HostInfo {
    sample_rate: AtomicF32,
    max_buffer: AtomicU32,
    inputs: AtomicU32,
    outputs: AtomicU32,
    last_buffer: AtomicU32,
    process_calls: AtomicU64,
}

const SESSION_VARS: &[&str] = &[
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "GDK_SCALE",
    "GDK_DPI_SCALE",
    "QT_SCALE_FACTOR",
];

pub struct SimpleTone {
    params: Arc<SimpleToneParams>,
    sample_rate: f32,
    phase: f32,
    peak_meter_decay_weight: f32,
    peak_meter: Arc<AtomicF32>,
    host: Arc<HostInfo>,
}

#[derive(Params)]
pub struct SimpleToneParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    /// Master gain control in dB
    #[id = "gain"]
    pub gain: FloatParam,

    /// Mute toggle
    #[id = "mute"]
    pub mute: BoolParam,

    /// Test tone generator enable
    #[id = "test_tone"]
    pub test_tone: BoolParam,

    /// Sine tone frequency in Hz
    #[id = "frequency"]
    pub frequency: FloatParam,
}

impl Default for SimpleTone {
    fn default() -> Self {
        Self {
            params: Arc::new(SimpleToneParams::default()),
            sample_rate: 44100.0,
            phase: 0.0,
            peak_meter_decay_weight: 1.0,
            peak_meter: Arc::new(AtomicF32::new(util::MINUS_INFINITY_DB)),
            host: Arc::new(HostInfo::default()),
        }
    }
}

impl Default for SimpleToneParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(420, 640),

            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(12.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 12.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            mute: BoolParam::new("Mute", false),

            test_tone: BoolParam::new("Test Tone (Sine)", false),

            frequency: FloatParam::new(
                "Frequency",
                440.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(1))
            .with_string_to_value(Arc::new(|s| {
                s.trim_end_matches(&[' ', 'h', 'H', 'z', 'Z']).parse().ok()
            })),
        }
    }
}

impl Plugin for SimpleTone {
    const NAME: &'static str = "SimpleTone";
    const VENDOR: &'static str = "AudioPlugin Test";
    const URL: &'static str = "https://github.com";
    const EMAIL: &'static str = "test@example.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        // Stereo in -> Stereo out
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        // Mono in -> Mono out
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
        // Generator / Synth mode (no input -> Stereo out)
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
    ];

    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let peak_meter = self.peak_meter.clone();
        let egui_state = params.editor_state.clone();
        let host = self.host.clone();
        let opened_at = std::time::Instant::now();

        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _state| {
                ResizableWindow::new("simple-tone-editor")
                    .min_size(Vec2::new(380.0, 560.0))
                    .show(egui_ctx, egui_state.as_ref(), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.heading(RichText::new("SimpleTone Test Plugin").size(18.0).strong());
                            ui.label(
                                RichText::new(format!(
                                    "Platform: {} | Arch: {}",
                                    std::env::consts::OS,
                                    std::env::consts::ARCH
                                ))
                                .size(11.0)
                                .color(Color32::from_rgb(140, 180, 240)),
                            );
                            ui.label(
                                RichText::new("Formats: CLAP & VST3")
                                    .size(11.0)
                                    .color(Color32::GRAY),
                            );
                        });

                        ui.separator();

                        ui.group(|ui| {
                            ui.label(RichText::new("Diagnostics").strong());
                            let ppp = egui_ctx.pixels_per_point();
                            let screen = egui_ctx.screen_rect().size();
                            let (state_w, state_h) = egui_state.size();
                            ui.label(format!(
                                "format: {} | version: {}",
                                setter.raw_context.plugin_api(),
                                env!("CARGO_PKG_VERSION")
                            ));
                            ui.label(format!("pixels_per_point: {ppp:.2}"));
                            ui.label(format!("window logical: {:.0} x {:.0}", screen.x, screen.y));
                            ui.label(format!(
                                "window physical: {:.0} x {:.0}",
                                screen.x * ppp,
                                screen.y * ppp
                            ));
                            ui.label(format!("editor state size: {state_w} x {state_h}"));
                            ui.label(format!(
                                "sample rate: {} | max buffer: {} | last buffer: {}",
                                host.sample_rate.load(Ordering::Relaxed),
                                host.max_buffer.load(Ordering::Relaxed),
                                host.last_buffer.load(Ordering::Relaxed),
                            ));
                            ui.label(format!(
                                "channels in/out: {}/{} | process calls: {}",
                                host.inputs.load(Ordering::Relaxed),
                                host.outputs.load(Ordering::Relaxed),
                                host.process_calls.load(Ordering::Relaxed),
                            ));
                            ui.label(format!(
                                "editor open: {:.1} s",
                                opened_at.elapsed().as_secs_f32()
                            ));
                            for var in SESSION_VARS {
                                let value = std::env::var(var).unwrap_or_else(|_| "-".into());
                                ui.label(format!("{var}: {value}"));
                            }
                        });
                        egui_ctx.request_repaint();

                        ui.add_space(4.0);

                        // Test Tone generator section
                        ui.group(|ui| {
                            ui.label(RichText::new("Test Tone Generator").strong());
                            ui.horizontal(|ui| {
                                ui.label("Enable Sine:");
                                ui.add(widgets::ParamSlider::for_param(&params.test_tone, setter));
                            });

                            ui.label("Sine Frequency:");
                            ui.add(widgets::ParamSlider::for_param(&params.frequency, setter));
                        });

                        ui.add_space(4.0);

                        // Master section
                        ui.group(|ui| {
                            ui.label(RichText::new("Master Output").strong());

                            ui.label("Gain:");
                            ui.add(widgets::ParamSlider::for_param(&params.gain, setter));

                            ui.horizontal(|ui| {
                                ui.label("Mute:");
                                ui.add(widgets::ParamSlider::for_param(&params.mute, setter));
                            });
                        });

                        ui.add_space(4.0);

                        // Output Peak Meter
                        let peak_val_linear = peak_meter.load(Ordering::Relaxed);
                        let peak_db = util::gain_to_db(peak_val_linear);
                        let peak_text = if peak_db > -60.0 {
                            format!("{peak_db:.1} dBFS")
                        } else {
                            "-inf dBFS".to_string()
                        };

                        let normalized_progress = ((peak_db + 60.0) / 60.0).clamp(0.0, 1.0);

                        ui.horizontal(|ui| {
                            ui.label("Output Level:");
                            ui.add(
                                ProgressBar::new(normalized_progress)
                                    .text(peak_text)
                                    .animate(false),
                            );
                        });
                    });
            },
        )
    }

    fn initialize(
        &mut self,
        audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.phase = 0.0;

        let channels = |n: Option<NonZeroU32>| n.map_or(0, NonZeroU32::get);
        self.host
            .sample_rate
            .store(buffer_config.sample_rate, Ordering::Relaxed);
        self.host
            .max_buffer
            .store(buffer_config.max_buffer_size, Ordering::Relaxed);
        self.host.inputs.store(
            channels(audio_io_layout.main_input_channels),
            Ordering::Relaxed,
        );
        self.host.outputs.store(
            channels(audio_io_layout.main_output_channels),
            Ordering::Relaxed,
        );

        self.peak_meter_decay_weight = 0.25f64
            .powf((buffer_config.sample_rate as f64 * PEAK_METER_DECAY_MS / 1000.0).recip())
            as f32;

        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let is_muted = self.params.mute.value();
        let test_tone_enabled = self.params.test_tone.value();

        let num_samples = buffer.samples();
        let num_channels = buffer.channels();
        self.host
            .last_buffer
            .store(num_samples as u32, Ordering::Relaxed);
        self.host.process_calls.fetch_add(1, Ordering::Relaxed);

        let mut max_abs_sample: f32 = 0.0;

        for sample_idx in 0..num_samples {
            let gain = self.params.gain.smoothed.next();
            let freq = self.params.frequency.smoothed.next();

            let tone_sample = if test_tone_enabled {
                let s = (self.phase * std::f32::consts::TAU).sin() * 0.25; // -12 dB safe level
                let phase_delta = freq / self.sample_rate;
                self.phase = (self.phase + phase_delta).fract();
                s
            } else {
                0.0
            };

            for channel_idx in 0..num_channels {
                let sample_ptr = &mut buffer.as_slice()[channel_idx][sample_idx];

                let input_sample = if test_tone_enabled {
                    tone_sample
                } else {
                    *sample_ptr
                };

                let processed_sample = if is_muted { 0.0 } else { input_sample * gain };

                *sample_ptr = processed_sample;

                let abs_val = processed_sample.abs();
                if abs_val > max_abs_sample {
                    max_abs_sample = abs_val;
                }
            }
        }

        // Update peak meter for GUI if editor is open
        if self.params.editor_state.is_open() {
            let current_peak = self.peak_meter.load(Ordering::Relaxed);
            let new_peak = if max_abs_sample > current_peak {
                max_abs_sample
            } else {
                current_peak * self.peak_meter_decay_weight
                    + max_abs_sample * (1.0 - self.peak_meter_decay_weight)
            };
            self.peak_meter.store(new_peak, Ordering::Relaxed);
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for SimpleTone {
    const CLAP_ID: &'static str = "org.testing.simple-tone";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Simple cross-platform audio test plugin (CLAP + VST3) for ARM64 & x86_64");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Instrument,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for SimpleTone {
    const VST3_CLASS_ID: [u8; 16] = *b"SimpleToneTester";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Tools,
        Vst3SubCategory::Generator,
    ];
}

nih_export_clap!(SimpleTone);
nih_export_vst3!(SimpleTone);
