extern crate sdl2;

use rand::Rng;
use sdl2::audio::AudioCallback;
use sdl2::audio::AudioDevice;
use sdl2::audio::AudioSpec;
use std::f32::MAX;
use std::time::Duration;
use std::time::Instant;
use std::{fs::File, io::Write};

use sdl2::audio::AudioQueue;

use crate::AUDIO_SAMPLE_RATE;
use crate::FPS;

const SAMPLES_PER_MS: f64 = AUDIO_SAMPLE_RATE as f64 / 1000.0;

const MAX_VOLUME: f64 = 1.0;
const ATTACK_DURATIONS: [u32; 16] = [
    2, 8, 16, 24, 38, 56, 68, 80, 100, 250, 500, 800, 1000, 3000, 5000, 8000,
];
const DECAY_DURATIONS: [u32; 16] = [
    6, 24, 48, 72, 114, 168, 204, 240, 300, 750, 1500, 2400, 3000, 9000, 15000, 24000,
];
const RELEASE_DURATIONS: [u32; 16] = [
    6, 24, 48, 72, 114, 168, 204, 240, 300, 750, 1500, 2400, 3000, 9000, 15000, 24000,
];

pub enum WaveForm {
    Triangle = 0,
    Sawtooth,
    Square,
    Noise,
}

pub struct Wave<'a> {
    period_samples: f64,
    phase_inc: f64,
    phase: f64,
    volume: f64,
    gen_function: fn(&mut Wave) -> f64,
    out_file: &'a mut File,

    // Triangle wave support
    prev: f64,
    y: f64,
    x: f64,
    r: f64,

    // Custom sounds
    use_custom_params: bool,
    attack_samples: u32,
    decay_samples: u32,
    sustain_samples: u32,
    release_samples: u32,
    sample_progress: u32,
    sample_inc: u32,
    sustain: f64,
}

pub fn default_wave(output_file: &mut File) -> Wave {
    Wave {
        period_samples: 0.0,
        phase_inc: 0.0,
        phase: 0.0,
        volume: 10_000.0,
        sustain: 10_000.0,
        gen_function: gen_triangle_wave,
        out_file: output_file,
        prev: 0.0,
        y: 0.0,
        x: 0.0,
        r: (-1.0 / (0.0025 * AUDIO_SAMPLE_RATE as f64)).exp(),
        use_custom_params: false,
        attack_samples: 0,
        decay_samples: 0,
        sustain_samples: 0,
        release_samples: 0,
        sample_progress: 0,
        sample_inc: 0,
    }
}

// See https://pbat.ch/sndkit/blep/
fn polyblep(dt: f64, mut t: f64) -> f64 {
    if t < dt {
        t /= dt;
        return t + t - t * t - 1.0;
    } else if t > 1.0 - dt {
        t = (t - 1.0) / dt;
        return t * t + t + t + 1.0;
    }

    return 0.0;
}

fn gen_square_wave(wave: &mut Wave) -> f64 {
    let mut value = if wave.phase <= 0.5 { 1.0 } else { -1.0 };
    value += polyblep(wave.phase_inc, wave.phase);
    value -= polyblep(wave.phase_inc, (wave.phase + 0.5) % 1.0);
    return value;
}

fn gen_sawtooth_wave(wave: &mut Wave) -> f64 {
    let mut value = (2.0 * wave.phase) - 1.0;
    let poly = polyblep(wave.phase_inc, wave.phase);
    value -= poly;
    return value;
}

// We generate the triangle wave by integrating the square wave with
// a leaky integrator and then applying a DC blocker. Again reference
// sndkit
fn gen_triangle_wave(wave: &mut Wave) -> f64 {
    let mut value = gen_square_wave(wave);

    // scale and integrate
    value *= 4.0 / wave.period_samples;
    value += wave.prev;
    wave.prev = value;

    // DC blocker
    wave.y = value - wave.x + wave.r * wave.y;
    wave.x = value;

    value = wave.y * 0.8;
    //println!(
    //    "value: {}, x: {}, y: {}, prev: {} ",
    //    value, wave.x, wave.y, wave.prev
    //);
    return value;
}

fn gen_noise(_wave: &mut Wave) -> f64 {
    return rand::thread_rng().gen_range(-1.0..1.0);
}

// Used to compute and discard the first few periods of triangle waves to skip
// the initial DC offset
fn precompute_cycles(wave: &mut Wave) {
    for _ in 0..((wave.period_samples) as i64 * 10) {
        (wave.gen_function)(wave);
        wave.increment_phase();
    }
}

impl Wave<'_> {
    fn increment_phase(&mut self) {
        self.phase += self.phase_inc;
        if self.phase > 1.0 {
            self.phase -= 1.0
        }
    }

    fn calculate_volume(&mut self) -> f64 {
        if !self.use_custom_params {
            return MAX_VOLUME;
        }

        // attack
        if self.sample_progress <= self.attack_samples {
            //dbg_println!("In attack");
            return self.volume * (self.sample_progress as f64 / self.attack_samples as f64);
        }

        // decay
        let decay_threshold = self.attack_samples;
        if self.sample_progress <= (decay_threshold + self.decay_samples) {
            //dbg_println!("In decay");
            return self.sustain
                + (self.volume - self.sustain)
                    * (1.0
                        - (self.sample_progress - decay_threshold) as f64
                            / self.decay_samples as f64);
        }

        // sustain
        let sustain_threshold = self.attack_samples + self.decay_samples;
        if self.sample_progress <= (sustain_threshold + self.sustain_samples) {
            //dbg_println!("In sustain");
            return self.sustain;
        }

        // release
        let release_threshold = self.attack_samples + self.decay_samples + self.sustain_samples;

        if self.sample_progress <= (release_threshold + self.release_samples) {
            //dbg_println!("In release");
            return self.sustain
                * (1.0
                    - (self.sample_progress - release_threshold) as f64
                        / self.release_samples as f64);
        }

        return 0.0;
    }
}

impl AudioCallback for Wave<'_> {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        for x in out.iter_mut() {
            self.sample_progress += self.sample_inc;
            let volume = self.calculate_volume();
            //println!("vol: {}", volume);
            *x = ((self.gen_function)(self) * volume) as f32;
            instr_dbg_println!(
                "Generated {} from phase {}, sample_progress {}, volume {}",
                *x,
                self.phase,
                self.sample_progress,
                volume
            );
            self.increment_phase();

            let integer_result = (*x * 10_000.0) as i16;
            //println!(
            //    "Volume: {}, phase: {}, result: {}, writing: {}",
            //    self.volume, self.phase, *x, integer_result
            //);
            self.out_file
                .write_all(&integer_result.to_le_bytes())
                .expect("Failed to write audio to file");
        }
    }
}

pub fn wave_form_from_num(index: u8) -> Result<WaveForm, String> {
    return match index {
        0 => Ok(WaveForm::Triangle),
        1 => Ok(WaveForm::Sawtooth),
        2 => Ok(WaveForm::Square),
        3 => Ok(WaveForm::Noise),
        _ => Err(String::from("Invalid wave type index")),
    };
}

pub struct AudioState<'a> {
    frequency: i32,
    duration: Duration,
    duration_ms: u16,
    total_duration_ms: u32,
    playing: bool,
    started_at: Instant,
    device: &'a mut AudioDevice<Wave<'a>>,
    use_custom_params: bool,
    attack: usize,
    decay: usize,
    release: usize,
    sustain: f64,
    volume: f64,
    wave_form: WaveForm,
}

impl AudioState<'_> {
    pub fn new<'a>(device: &'a mut AudioDevice<Wave<'a>>) -> AudioState<'a> {
        return AudioState {
            frequency: 0,
            duration: Duration::new(0, 0),
            duration_ms: 0,
            total_duration_ms: 0,
            playing: false,
            started_at: Instant::now(),
            device: device,
            use_custom_params: false,
            attack: 0,
            decay: 0,
            release: 0,
            sustain: MAX_VOLUME,
            volume: MAX_VOLUME,
            wave_form: WaveForm::Square,
        };
    }

    pub fn play_sound(&mut self, frequency: u16, duration: u16) {
        self.use_custom_params = false;
        self.wave_form = WaveForm::Square;
        self.volume = MAX_VOLUME;
        self.frequency = frequency as i32;
        self.duration = Duration::new(0, (duration as u32) * 1_000_000);
        self.duration_ms = duration;
        self.total_duration_ms = duration as u32;
        self.update_wave();
        self.start();
    }

    pub fn play_custom_sound(&mut self, frequency: u16, duration: u16) {
        let sustain_duration =
            if ATTACK_DURATIONS[self.attack] + DECAY_DURATIONS[self.decay] < duration as u32 {
                duration as u32 - ATTACK_DURATIONS[self.attack] - DECAY_DURATIONS[self.decay]
            } else {
                0
            };

        let total_duration = ATTACK_DURATIONS[self.attack]
            + DECAY_DURATIONS[self.decay]
            + (sustain_duration)
            + RELEASE_DURATIONS[self.release];
        self.total_duration_ms = total_duration;
        self.duration_ms = duration;
        self.frequency = frequency as i32;
        self.duration = Duration::new(0, (total_duration) * 1_000_000);
        self.use_custom_params = true;
        instr_dbg_println!(
            "Playing {}hz for {}ms, dur: {}, atk: {}ms, dec: {}ms, sus: {}ms, rel: {}ms",
            self.frequency,
            total_duration,
            duration,
            ATTACK_DURATIONS[self.attack],
            DECAY_DURATIONS[self.decay],
            sustain_duration,
            RELEASE_DURATIONS[self.release]
        );

        self.update_wave();
        self.start();
    }

    pub fn set_params(
        &mut self,
        attack: u8,
        decay: u8,
        sustain: u8,
        release: u8,
        volume: u8,
        wave_type: u8,
    ) -> Result<(), String> {
        self.attack = attack as usize;
        self.decay = decay as usize;
        self.release = release as usize;
        self.sustain = MAX_VOLUME / (2.0 * (16.0 - sustain as f64));
        self.volume = MAX_VOLUME / (2.0 * (16.0 - volume as f64));
        self.wave_form = wave_form_from_num(wave_type)?;
        Ok(())
    }

    pub fn update_wave(&mut self) {
        self.started_at = Instant::now();

        let mut wave = self.device.lock();

        wave.phase_inc = self.frequency as f64 / AUDIO_SAMPLE_RATE as f64;
        wave.period_samples = AUDIO_SAMPLE_RATE as f64 / self.frequency as f64;
        wave.use_custom_params = self.use_custom_params;
        wave.sample_progress = 0;
        wave.sample_inc = 1;

        if self.use_custom_params {
            wave.volume = self.volume;
            wave.sustain = self.sustain;

            wave.attack_samples = (SAMPLES_PER_MS * ATTACK_DURATIONS[self.attack] as f64) as u32;
            wave.decay_samples = (SAMPLES_PER_MS * DECAY_DURATIONS[self.decay] as f64) as u32;
            wave.release_samples = (SAMPLES_PER_MS * RELEASE_DURATIONS[self.release] as f64) as u32;

            let sustain_duration = if ATTACK_DURATIONS[self.attack] + DECAY_DURATIONS[self.decay]
                < self.duration_ms as u32
            {
                self.duration_ms as u32
                    - ATTACK_DURATIONS[self.attack]
                    - DECAY_DURATIONS[self.decay]
            } else {
                0
            };

            wave.sustain_samples = (SAMPLES_PER_MS * sustain_duration as f64) as u32;

            instr_dbg_println!(
                "atk: {}, dec: {}, sus: {}, rel: {}",
                wave.attack_samples,
                wave.decay_samples,
                wave.sustain_samples,
                wave.release_samples
            );
        }

        match self.wave_form {
            WaveForm::Triangle => {
                instr_dbg_println!("Selected triangle wave");
                wave.gen_function = gen_triangle_wave;
                // If we have a prev value then we don't need to precompute
                if !(wave.prev != 0.0 || wave.x != 0.0 || wave.y != 0.0) {
                    precompute_cycles(&mut wave);
                }
            }
            WaveForm::Square => {
                instr_dbg_println!("Selected square wave");
                wave.gen_function = gen_square_wave;
            }
            WaveForm::Sawtooth => {
                instr_dbg_println!("Selected sawtooth wave");
                wave.gen_function = gen_sawtooth_wave;
            }
            WaveForm::Noise => {
                instr_dbg_println!("Selected noise wave");
                wave.gen_function = gen_noise;
            }
        }
        instr_dbg_println!(
            "Playing {}hz for {}ms, ~{} samples",
            self.frequency,
            self.total_duration_ms,
            SAMPLES_PER_MS * (self.total_duration_ms as f64),
        );

        self.playing = true;
    }

    pub fn is_finished(&mut self) -> bool {
        let passed_duration = self.started_at.elapsed();
        return self.playing && passed_duration > self.duration;
    }

    pub fn clear(&mut self) {
        self.frequency = 0;
        self.duration = Duration::new(0, 0);
        self.duration_ms = 0;
        self.playing = false;
        self.started_at = Instant::now();
        self.use_custom_params = false;
        self.attack = 0;
        self.decay = 0;
        self.release = 0;
        self.sustain = MAX_VOLUME;
        self.volume = MAX_VOLUME;
        self.wave_form = WaveForm::Square;
        //self.device.pause();
        self.device.pause();
        instr_dbg_println!("finished and clearing");

        let mut wave = self.device.lock();

        wave.period_samples = 0.0;
        wave.phase_inc = 0.0;
        wave.phase = 0.0;
        wave.volume = 0.0;
        wave.sustain = 0.0;
        wave.prev = 0.0;
        wave.x = 0.0;
        wave.y = 0.0;
        wave.use_custom_params = false;
        wave.attack_samples = 0;
        wave.decay_samples = 0;
        wave.sustain_samples = 0;
        wave.release_samples = 0;
        wave.sample_progress = 0;
        wave.sample_inc = 0;

        wave.out_file.sync_all().expect("Failed to sync audio file");
    }

    pub fn start(&mut self) {
        self.device.resume();
    }
}
