extern crate sdl2;

use sdl2::audio::AudioCallback;
use sdl2::audio::AudioDevice;
use sdl2::audio::AudioSpec;
use std::time::Duration;
use std::time::Instant;
use std::{fs::File, io::Write};

use sdl2::audio::AudioQueue;

use crate::AUDIO_SAMPLE_RATE;
use crate::FPS;

pub enum WaveType {
    Square,
    Sawtooth,
    Triangle,
    Noise,
}

pub struct Wave<'a> {
    pub period: i16,
    pub slope: i16,
    pub phase_inc: i16,
    pub phase: i16,
    pub volume: i16,
    pub wave_form: WaveType,
    pub out_file: &'a mut File,
}

fn gen_square_wave(period: i16, phase: i16, volume: i16) -> i16 {
    return if (phase / period) % 2 == 0 {
        volume
    } else {
        -volume
    };
}

fn gen_sawtooth_wave(period: i16, slope: i16, phase: i16, volume: i16) -> i16 {
    return if phase < (period / 2) {
        slope * phase
    } else {
        (slope * phase) - volume
    };
}

impl AudioCallback for Wave<'_> {
    type Channel = i16;

    fn callback(&mut self, out: &mut [i16]) {
        for x in out.iter_mut() {
            let result: i16;
            match self.wave_form {
                WaveType::Square => {
                    result = gen_square_wave(self.period, self.phase, self.volume);
                }
                WaveType::Sawtooth => {
                    result = gen_sawtooth_wave(self.period, self.slope, self.phase, self.volume);
                }
                WaveType::Triangle => {
                    result = gen_sawtooth_wave(self.period, self.slope, self.phase, self.volume);
                }
                WaveType::Noise => {
                    result = gen_sawtooth_wave(self.period, self.slope, self.phase, self.volume);
                }
            }
            *x = result;
            self.phase = (self.phase + self.phase_inc) % self.period;
            //println!(
            //    "Volume: {}, phase: {}, result: {}",
            //    self.volume, self.phase, result
            //);
            self.out_file
                .write_all(&result.to_le_bytes())
                .expect("Failed to write audio to file");
        }
    }
}

//fn gen_sawtooth_wave(
//    initial_pos: i32,
//    sample_count: i32,
//    frequency: u16,
//    out_file: &mut File,
//) -> (Vec<i16>, i32) {
//    // Generate a sawtooth wave
//    let tone_volume = 10_000i16;
//
//    // period will always be a small positive number so casting to i16 is fine
//    let period: i32 = (AUDIO_SAMPLE_RATE / i32::from(frequency))
//        .try_into()
//        .expect("Frequency was out of bounds");
//    let mut result: Vec<i16> = Vec::new();
//
//    let slope = tone_volume / (period as i16);
//    let mut x = initial_pos;
//    while x < (initial_pos + sample_count) {
//        let sample = if (x % period) < i32::from(period) / 2 {
//            slope * (x % period) as i16
//        } else {
//            (slope * (x % period) as i16) - tone_volume
//        }; //dbg_println!("Sample: {}", sample);
//        out_file
//            .write_all(&sample.to_le_bytes())
//            .expect("Failed to write audio to file");
//        result.push(sample);
//        x += 1;
//    }
//
//    (result, x % period)
//}
//
//fn gen_square_wave(bytes_to_write: i32, frequency: u16) -> Vec<i16> {
//    // Generate a square wave
//    let tone_volume = 1_000i16;
//    let period = AUDIO_SAMPLE_RATE / i32::from(frequency);
//    let sample_count = bytes_to_write;
//    let mut result = Vec::new();
//
//    for x in 0..sample_count {
//        result.push(if (x / period) % 2 == 0 {
//            tone_volume
//        } else {
//            -tone_volume
//        });
//    }
//    result
//}

pub struct AudioState<'a> {
    frequency: i16,
    pub duration: Duration,
    playing: bool,
    started_at: Instant,
    device: &'a mut AudioDevice<Wave<'a>>,
}

impl AudioState<'_> {
    pub fn new<'a>(device: &'a mut AudioDevice<Wave<'a>>) -> AudioState<'a> {
        return AudioState {
            frequency: 0,
            duration: Duration::new(0, 0),
            playing: false,
            started_at: Instant::now(),
            device: device,
        };
    }

    pub fn set_params(&mut self, frequency: u16, duration: u16) {
        self.frequency = frequency as i16;
        self.duration = Duration::new(0, (duration as u32) * 1_000_000);
        self.started_at = Instant::now();
        self.playing = true;

        let mut wave = self.device.lock();

        // period will always be a small positive number so casting to i16 is fine
        wave.period = (AUDIO_SAMPLE_RATE / i32::from(self.frequency))
            .try_into()
            .expect("Frequency was out of bounds");

        wave.slope = wave.volume / wave.period;

        println!("Set frequency: {}, period: {}", frequency, wave.period);
    }

    pub fn is_finished(&mut self) -> bool {
        let passed_duration = self.started_at.elapsed();
        return self.playing && passed_duration > self.duration;
    }

    pub fn clear(&mut self) {
        self.playing = false;
        self.device.pause();

        let mut a = self.device.lock();
        a.out_file.sync_all().expect("Failed to sync audio file");
    }

    pub fn start(&mut self) {
        self.device.resume();
    }

    pub fn set_waveform(&mut self) {
        let mut a = self.device.lock();
        a.wave_form = WaveType::Sawtooth
    }
}
