#[macro_export]
macro_rules! dbg_println {
    ($($arg:tt)*) => (#[cfg(debug_assertions)] println!($($arg)*));
}
extern crate sdl2;

mod audio;
mod cpu;
mod renderer;

use binrw::binread;
use binrw::io::{Cursor, Seek};
use binrw::BinReaderExt; // extension traits for use with readers and writers // A no_std reimplementation of std::io

use clap::Parser;
use cpu::CPU;
use rand::Rng;
use renderer::Renderer;
use sdl2::audio::AudioSpecDesired;
use std::fs::File;
use std::io::Read;

const GRID_X_SIZE: u32 = 320;
const GRID_Y_SIZE: u32 = 240;
const DOT_SIZE_IN_PXS: u32 = 2;
const CLOCK_RATE: u32 = 1000000;
const FPS: u32 = 60;
const FRAME_CYCLES: u32 = CLOCK_RATE / FPS;
const AUDIO_SAMPLE_RATE: i32 = 48_000;

#[binread]
pub struct ROMHeader {
    magic_number: [u8; 4],
    reserved: u8,
    specification_version: u8,
    rom_size: u32,
    start_address: [u8; 2],
    rom_crc: [u8; 4],
}

pub fn parse_rom(path: String) -> Result<(), String> {
    let mut rom = File::open(path).expect("Should have been able to open the file");

    let mut header = [0; 16];

    rom.read(header.as_mut_slice())
        .expect("Should have been able to read the rom");

    let has_header = [header[0], header[1], header[2], header[3]]
        == ['C' as u8, 'H' as u8, '1' as u8, '6' as u8];

    let mut mem = [0; 65536];
    if has_header {
        rom.seek(std::io::SeekFrom::Start(16))
            .expect("Should have been able to seek to 16 in the rom");
    } else {
        rom.seek(std::io::SeekFrom::Start(0))
            .expect("Should have been able to seek to 16 in the rom");
    }
    rom.read(mem.as_mut_slice())
        .expect("Should have been able to read the rom");

    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;
    let audio_subsystem = sdl_context.audio()?;

    let desired_spec = AudioSpecDesired {
        freq: Some(AUDIO_SAMPLE_RATE),
        channels: Some(1),
        // mono  -
        samples: None, // default sample size
    };

    let mut output_file =
        File::create("cpu_audio_output").expect("Failed to open audio file for writing");

    let mut audio_device =
        audio_subsystem.open_playback(None, &desired_spec, |spec| audio::Wave {
            period: 0,
            slope: 0,
            phase_inc: 1,
            phase: 0,
            volume: 10_000,
            wave_form: audio::WaveType::Sawtooth,
            out_file: &mut output_file,
        })?;

    let mut audio_state = audio::AudioState::new(&mut audio_device);

    let mut event_pump = sdl_context.event_pump()?;

    let window = video_subsystem
        .window(
            "snake-game",
            GRID_X_SIZE * DOT_SIZE_IN_PXS,
            GRID_Y_SIZE * DOT_SIZE_IN_PXS,
        )
        .position_centered()
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;

    let mut renderer = Renderer::new(window)?;

    let mut cpu = CPU::new(&mut mem, &mut event_pump, &mut audio_state);

    if has_header {
        let mut reader = Cursor::new(header);
        let header: ROMHeader = reader.read_le().unwrap();
        dbg_println!(
            "Header: magic_number: {:#02X?}\nreserved: {:#02X?}\nspec: {:#02X?}\nrom_size: {:#02X?}\nstart_address: {:#02X?}\ncrc: {:#02X?}",
            header.magic_number,
            header.reserved,
            header.specification_version,
            header.rom_size,
            header.start_address,
            header.rom_crc
        );

        cpu.set_pc(header.start_address);
    } else {
        // println!("No header");
    }

    cpu.init();
    cpu.run(&mut renderer)?;
    Ok(())
}

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long)]
    rom_path: String,
}

pub fn main() -> Result<(), String> {
    let args = Args::parse();
    parse_rom(args.rom_path)?;

    Ok(())
}
