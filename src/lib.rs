#![allow(unused)]
use nih_plug::prelude::*;
use nih_plug_vizia::ViziaState;
use std::sync::Arc;
use rand::Rng;

mod editor;

fn gen_perlin_noise(x: i32) -> f32 {
    let xshift: i128 = ((x << 13) ^ x) as i128;
    let a0: i128 = (xshift * xshift * 15731_i128 + 789221_i128);
    let a1: i128 = (xshift * a0 + 1376312589_i128);
    let xscaled: f32 = (a1 & 0x7fffffff) as f32;
    let res = (1.0 - xscaled as f32 / 1073741824 as f32);

    res
}

struct Crrshrr {
    params: Arc<CrrshrrParams>,
    counter: i32,
    offset: usize,
}

#[derive(Params)]
struct CrrshrrParams {
    /// The editor state, saved together with the parameter state so the custom scaling can be
    /// restored.
    #[persist = "editor_state"]
    editor_state: Arc<ViziaState>,
    #[id = "bits"]
    pub bits: FloatParam,
    #[id = "rate"]
    pub rate: IntParam,
    #[id = "rand"]
    pub rand: IntParam,
    #[id = "rand_rate"]
    pub rand_rate: IntParam,
    #[id = "noise"]
    pub noise: FloatParam,
}

impl Default for Crrshrr {
    fn default() -> Self {
        Self {
            params: Arc::new(CrrshrrParams::default()),
            counter: 0,
            offset: 0,
        }
    }
}

impl Default for CrrshrrParams {
    fn default() -> Self {
        Self {
            editor_state: editor::default_state(),

            /*
            Bit reduction. 
            Anything above 16.0 is pointless, and anything below 4.0 turns 
            into outbursts of noise that are kind of useless.
             */
            bits: FloatParam::new(
                "bits",
                16.0,
                FloatRange::Linear { min: 4.0, max: 16.0, },
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            // .with_unit(" bits")
            .with_step_size(0.1),

            /*
            Sample rate reduction.
            
            TODO: maybe display values as actual sample rate kHz.
             */
            rate: IntParam::new(
                "rate",
                1,
                IntRange::Linear { min: 1, max: 50, }
            ),

            /*
            Offsets the sample rate's sample & hold logic. 
             */
            rand: IntParam::new(
                "rand",
                0,
                IntRange::Linear { min: 0, max: 100, },
            ),

            rand_rate: IntParam::new(
                "rand rate",
                0,
                IntRange::Linear { min: 0, max: 64, },
            ),

            /*
            This is really more of a gain control for the rand-based noise that gets added to the sample 
            data during the bit crushing phase.
             */
            noise: FloatParam::new(
                "noise", 
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_step_size(0.1),
        }
    }
}

impl Plugin for Crrshrr {
    const NAME: &'static str = "crrshrr";
    const VENDOR: &'static str = "LASHLIGHT";
    const URL: &'static str = env!("CARGO_PKG_HOMEPAGE");
    const EMAIL: &'static str = "lashlight@proton.me";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // The first audio IO layout is used as the default. The other layouts may be selected either
    // explicitly or automatically by the host or the user depending on the plugin API/backend.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),

        aux_input_ports: &[],
        aux_output_ports: &[],

        // Individual ports and the layout as a whole can be named here. By default these names
        // are generated as needed. This layout will be called 'Stereo', while a layout with
        // only one input and output channel would be called 'Mono'.
        names: PortNames::const_default(),
    }];


    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;

    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    // If the plugin can send or receive SysEx messages, it can define a type to wrap around those
    // messages here. The type implements the `SysExMessage` trait, which allows conversion to and
    // from plain byte buffers.
    type SysExMessage = ();
    // More advanced plugins can use this to run expensive background tasks. See the field's
    // documentation for more information. `()` means that the plugin does not have any background
    // tasks.
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(
            self.params.clone(),
            self.params.editor_state.clone(),
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        _buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // Resize buffers and perform other potentially expensive initialization operations here.
        // The `reset()` function is always called right after this function. You can remove this
        // function if you do not need it.
        true
    }

    fn reset(&mut self) {
        // Reset buffers and envelopes here. This can be called from the audio thread and may not
        // allocate. You can remove this function if you do not need it.
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {

        /** This is the old way, kept here for reference...
        
        // There are 1024 slices in buffer, eahc of length 2
        for (idx, channel_samples) in buffer.iter_samples().enumerate() {
            let mut snh: f32 = 0.0;

            // println!("channel_sampled idx={}, len={}", idx, channel_samples.len());
            
            for (ch, sample) in channel_samples.into_iter().enumerate() {
                // println!("  channel={}, sample={}", ch, sample);

                let bits_value: f32 = self.params.bits.smoothed.next();
                let bits: f32 = (2.0 as f32).powf(bits_value);
                let sample_scaled: f32 = bits * (0.5 * *sample + 0.5);
                let sample_rounded: f32 = sample_scaled.floor();
                let sample_rescaled: f32 = 2.0 * (sample_rounded / bits) - 1.0;
                // *sample = sample_rescaled;

                // set _ > 0 for now to avoid silence...
                if self.params.rate.smoothed.next() > 1 {
                    if ch % self.params.rate.smoothed.next() as usize != 0 {
                        // let b: f32 = channel_samples.get_unchecked_mut(idx % rate).;
                        // *sample = 
                        snh = sample_rescaled;
                    } else {
                        snh = *sample;
                    }
                }

                *sample = snh;
            }
        }
        */

        // Get the raw data as a slice 'channel -> [samples]'
        let output = buffer.as_slice();

        if self.counter > self.params.rand_rate.value() {
            self.counter = 0;
            // The offset value set to a random number between 0 and the current "rand" value, 
            // or 0 when "rand" is also at 0. This is due to an error that 'gen_range' throws 
            // when the range is '0..0'.
            self.offset = if self.params.rand.value() > 0 { 
                    rand::thread_rng().gen_range(0..(self.params.rand.value() as usize)) 
                } else {
                    0
                };
        } else {
            self.counter += 1;
        }
        
        for channel in 0..output.len() {
            // The current channel's sample data.
            let data: &mut [f32] = output[channel];

            for i in 0..data.len() {
                // Bit crush.
                let bits_value: f32 = self.params.bits.smoothed.next();
                let bits: f32 = (2.0 as f32).powf(bits_value);

                // Generate rand noise.
                // let noise = (rand::thread_rng().gen_range(0.0..2.0) * self.params.noise.smoothed.next());
                
                // Generate Perlin noise.
                let noise = gen_perlin_noise(rand::thread_rng().gen_range(0..data.len() as i32)) * 
                    self.params.noise.smoothed.next();
                // Scale down with added noise.
                let sample_scaled: f32 = bits * (0.5 * data[i] + 0.5) + noise;
                // Round down.
                let sample_rounded: f32 = sample_scaled.floor();
                // Scale up.
                let mut sample_rescaled: f32 = 2.0 * (sample_rounded / bits) - 1.0;
                // Add the data back.
                data[i] = sample_rescaled;

                // Sample & hold.
                if self.params.rate.smoothed.next() > 1 {
                    let j: usize = i % ((self.params.rate.smoothed.next() as usize) + self.offset);

                    // This is the first iteration of the sample & hold algo, before the offset was 
                    // introduced.
                    // 
                    // if (j != 0) {
                    //     data[i] = data[i - j];
                    // }

                    // Since the offset can point to out of bound indexes, this makes sure that it 
                    // stays within the range of the data array (0..1023).
                    if (j > 0 && j < data.len()) {
                        data[i] = data[i - j];
                    }
                }
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for Crrshrr {
    const CLAP_ID: &'static str = "com.lashlight.crrshrr";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A simple bit crusher.");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;

    // Don't forget to change these features
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Stereo];
}

impl Vst3Plugin for Crrshrr {
    const VST3_CLASS_ID: [u8; 16] = *b"lashlightcrrshrr";

    // And also don't forget to change these categories
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Distortion];
}

nih_export_clap!(Crrshrr);
nih_export_vst3!(Crrshrr);
