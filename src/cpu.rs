use std::fs::File;
use std::thread;
use std::time::{Duration, Instant};

use rand::Rng;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::EventPump;

use crate::audio::AudioState;
use crate::renderer::Renderer;
use crate::{audio, FRAME_CYCLES};
use byteorder::{ReadBytesExt, WriteBytesExt, LE};

const SCREEN_SIZE_X: u16 = 320;
const SCREEN_SIZE_Y: u16 = 240;
const SCREEN_BUF_SIZE: usize = SCREEN_SIZE_X as usize * SCREEN_SIZE_Y as usize;
const FRAME_DURATION: Duration = Duration::new(0, 1_000_000_000u32 / 60);

type Instruction = [u8; 4];

fn hhll(instruction: &Instruction) -> u16 {
    return (&instruction[2..4])
        .read_u16::<LE>()
        .expect("Failed to read instruction hhll");
}

fn rx_ry(instruction: &Instruction) -> (usize, usize) {
    return (
        usize::from(instruction[1] & 0xF),
        usize::from((instruction[1] & 0xF0) >> 4),
    );
}

fn rx(instruction: &Instruction) -> usize {
    return usize::from(instruction[1] & 0xF);
}

fn rx_ry_rz(instruction: &Instruction) -> (usize, usize, usize) {
    return (
        usize::from(instruction[1] & 0xF),
        usize::from((instruction[1] & 0xF0) >> 4),
        usize::from(instruction[2] & 0xF),
    );
}

fn n(instruction: &Instruction) -> u8 {
    return instruction[2] & 0xF;
}

fn nop(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("noop");
    Ok(())
}

fn error(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("Invalid command {:#02X?}", instruction[0]);
    Err(String::from("Invalid command"))
}

fn cls(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("cls");
    state.graphics.bg = 0;
    state.screen.iter_mut().for_each(|m| *m = 0);
    Ok(())
}

fn vblnk(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    //instr_dbg_println!("vblnk");
    // Skip to frame render if we hit vblank
    if !state.vblnk {
        //state.cycles = FRAME_CYCLES
        state.pc -= 4;
    }
    Ok(())
}

fn bgc_n(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    //instr_dbg_println!("bgc_n");
    state.graphics.bg = instruction[2] & 0xF;
    instr_dbg_println!("Set bg to {:#02X?}", state.graphics.bg);
    Ok(())
}

fn spr_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    //instr_dbg_println!("spr_hhll");
    state.graphics.spritew = instruction[2];
    state.graphics.spriteh = instruction[3];
    instr_dbg_println!(
        "Set spriteh: {:#02X?}, spritew: {:#02X?}",
        state.graphics.spriteh,
        state.graphics.spritew
    );

    Ok(())
}

fn drw_rx_ry_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    let (rx, ry) = rx_ry(instruction);
    let sprite_addr = hhll(instruction);

    //instr_dbg_println!("drw_r{:#02X}_r{:#02X}_{:#04X}", rx, ry, sprite_addr);
    draw_sprite(state, state.registers[rx], state.registers[ry], sprite_addr);
    Ok(())
}

fn drw_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    //instr_dbg_println!("drw_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    draw_sprite(
        state,
        state.registers[rx],
        state.registers[ry],
        state.registers[rz] as u16,
    );
    Ok(())
}

fn rnd_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("rnd_rx_hhll");
    let max = (hhll(instruction) as u32) + 1;
    state.registers[rx(instruction)] = (rand::thread_rng().gen_range(0..(max)) & 0xFFFF) as i16;
    instr_dbg_println!(
        "Generated random number {}, from 0 to {}",
        state.registers[rx(instruction)],
        max - 1
    );
    Ok(())
}
fn flip(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("flip");
    let flip = hhll(instruction) >> 0x8;
    state.graphics.vflip = (flip & 0x1) != 0;
    state.graphics.hflip = ((flip >> 1) & 0x1) != 0;
    Ok(())
}
fn snd0(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("snd0");
    state.audio_state.clear();
    Ok(())
}
fn snd1_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("snd1_hhll");
    //dbg_println!("Playing for {} ms", hhll(instruction));

    state.audio_state.play_sound(500, hhll(instruction));

    Ok(())
}
fn snd2_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("snd2_hhll");
    state.audio_state.play_sound(1000, hhll(instruction));

    Ok(())
}
fn snd3_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("snd3_hhll");
    state.audio_state.play_sound(1500, hhll(instruction));
    Ok(())
}

fn snp_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("snp_rx_hhll");
    let rx = rx(instruction);
    let addr = (state.registers[rx] as usize) & 0xFFFF;
    let freq = load_mem(state, addr);
    instr_dbg_println!("addr: {:#02X}, hz: {}", addr, freq);
    instr_dbg_println!("freq: {}hz", freq);
    state.audio_state.play_custom_sound(freq, hhll(instruction));

    state.audio_state.start();
    Ok(())
}

fn sng_ad_vtsr(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("sng_ad_vtsr");
    let attack = (instruction[1] & 0xF0) >> 4;
    let decay = instruction[1] & 0xF;
    let sustain = (instruction[2] & 0xF0) >> 4;
    let release = instruction[2] & 0xF;
    let volume = (instruction[3] & 0xF0) >> 4;
    let wave_type = instruction[3] & 0xF;

    state
        .audio_state
        .set_params(attack, decay, sustain, release, volume, wave_type)?;

    instr_dbg_println!(
        "attack: {}, decay: {}, sustain: {}, release: {}, volume: {}, wave_type: {}",
        attack,
        decay,
        sustain,
        release,
        volume,
        wave_type
    );
    Ok(())
}

fn jmp_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    //instr_dbg_println!("jmp_hhll");
    state.pc = hhll(instruction);
    //instr_dbg_println!("Set pc to {:#02X?}", state.pc);

    Ok(())
}

fn jmc_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("jmc_hhll");
    if state.flags.C {
        state.pc = hhll(instruction);
    }
    Ok(())
}

fn jx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!(
        "j{:#02X?}_{:#04X?}",
        instruction[1] & 0xF,
        hhll(instruction)
    );
    instr_dbg_println!("{:?}", state.flags);
    instr_dbg_println!("{:?}", test_cond(state, instruction)?);
    if test_cond(state, instruction)? {
        state.pc = hhll(instruction);
        instr_dbg_println!("Set pc to {:#02X?}", state.pc);
    }
    Ok(())
}

fn jme_rx_ry_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    let (rx, ry) = rx_ry(instruction);

    instr_dbg_println!("jme_r{:#02X}_r{:#02X}_{:#04X?}", rx, ry, hhll(instruction));

    if state.registers[rx] == state.registers[ry] {
        state.pc = hhll(instruction);
    }
    Ok(())
}

fn call_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("call_hhll");
    store_mem(state, state.pc, state.sp);
    state.sp += 2;
    state.pc = hhll(instruction);
    state.stack.push(state.pc);
    instr_dbg_println!("Set pc to {:#02X?}", state.pc);
    Ok(())
}
fn ret(state: &mut CPU, _instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("ret");
    state.sp -= 2;
    let addr = state.sp;
    state.pc = load_mem(state, addr);
    state.stack.pop();
    Ok(())
}
fn jmp_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("jmp_rx");
    let (rx, _) = rx_ry(instruction);
    state.pc = state.registers[rx] as u16;
    Ok(())
}
fn cx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!(
        "c{:#02X?}_{:#04X?}",
        instruction[1] & 0xF,
        hhll(instruction)
    );
    if test_cond(state, instruction)? {
        store_mem(state, state.pc, state.sp);
        state.sp += 2;
        state.pc = hhll(instruction);
        state.stack.push(state.pc);
        instr_dbg_println!("Set pc to {:#02X?}", state.pc);
    }
    Ok(())
}
fn call_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("call_rx");
    let rx = rx(instruction);
    store_mem(state, state.pc, state.sp);
    state.sp += 2;
    state.pc = state.registers[rx] as u16;
    state.stack.push(state.pc);
    instr_dbg_println!("Set pc to {:#02X?}", state.pc);
    Ok(())
}
fn ldi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("ldi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    state.registers[rx] = hhll(instruction) as i16;
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn ldi_sp_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("ldi_sp_hhll");
    state.sp = hhll(instruction) as usize;
    Ok(())
}

fn ldm_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("ldm_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    state.registers[rx] = load_mem(state, hhll(instruction) as usize) as i16;

    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}

fn ldm_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("ldm_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    state.registers[rx] = load_mem(state, state.registers[ry] as usize) as i16;

    instr_dbg_println!(
        "Set register {:#02X?} to [register {:#02X?}] ([{:#02X?}]): {:#02X?}",
        rx,
        ry,
        state.registers[ry],
        state.registers[rx]
    );
    Ok(())
}
fn mov_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("mov_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    state.registers[rx] = state.registers[ry];
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}

fn stm_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("stm_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let addr = hhll(instruction) as usize;
    store_mem(state, state.registers[rx] as u16, addr);
    Ok(())
}

fn stm_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("stm_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    store_mem(
        state,
        state.registers[rx] as u16,
        state.registers[ry] as usize,
    );
    Ok(())
}

fn addi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("addi_rx_hhll");
    instr_dbg_println!(
        "{:x}: {:x} {:x} {:x} {:x}",
        state.pc,
        instruction[0],
        instruction[1],
        instruction[2],
        instruction[3]
    );

    let (rx, _) = rx_ry(instruction);
    let val = hhll(instruction) as i16;
    instr_dbg_println!("Adding {} to {}", state.registers[rx], val);
    state.registers[rx] = op_add(state, state.registers[rx], val);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    instr_dbg_println!("Flags: {:?}", state.flags);
    Ok(())
}
fn add_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("add_rx_ry");
    let (rx, ry) = rx_ry(instruction);

    state.registers[rx] = op_add(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn add_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("add_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    state.registers[rz] = op_add(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rz, state.registers[rz]);
    Ok(())
}
fn subi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("subi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = ((u16::from(instruction[3]) << 0x8) + u16::from(instruction[2])) as i16;

    state.registers[rx] = op_sub(state, state.registers[rx], val);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn sub_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("sub_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    state.registers[rx] = op_sub(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn sub_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("sub_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    state.registers[rz] = op_sub(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rz, state.registers[rz]);

    Ok(())
}
fn cmpi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("cmpi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = ((u16::from(instruction[3]) << 0x8) + u16::from(instruction[2])) as i16;
    op_sub(state, state.registers[rx], val);
    instr_dbg_println!("{:?}", state.flags);
    Ok(())
}
fn cmp_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("cmp_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    op_sub(state, state.registers[rx], state.registers[ry]);
    Ok(())
}
fn andi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("andi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = ((u16::from(instruction[3]) << 0x8) + u16::from(instruction[2])) as i16;
    and_flags(state, state.registers[rx], val);
    state.registers[rx] &= val;
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn and_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("and_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    and_flags(state, state.registers[rx], state.registers[ry]);
    state.registers[rx] &= state.registers[ry];
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn and_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("and_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    and_flags(state, state.registers[rx], state.registers[ry]);
    state.registers[rz] = state.registers[rx] & state.registers[ry];
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rz]);

    Ok(())
}
fn tsti_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("tsti_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    and_flags(state, state.registers[rx], hhll(instruction) as i16);

    Ok(())
}
fn tst_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("tsti_rx_hhll");
    let (rx, ry) = rx_ry(instruction);
    and_flags(state, state.registers[rx], state.registers[ry]);

    Ok(())
}
fn ori_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("ori_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = ((u16::from(instruction[3]) << 0x8) + u16::from(instruction[2])) as i16;
    state.registers[rx] |= val;
    set_flags_z_n(state, state.registers[rx]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn or_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("or_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    state.registers[rx] |= state.registers[ry];
    set_flags_z_n(state, state.registers[rx]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn or_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("or_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    state.registers[rz] = state.registers[rx] | state.registers[ry];
    set_flags_z_n(state, state.registers[rz]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rz]);
    Ok(())
}
fn xori_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("xori_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = ((u16::from(instruction[3]) << 0x8) + u16::from(instruction[2])) as i16;
    state.registers[rx] ^= val;
    set_flags_z_n(state, state.registers[rx]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn xor_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("xor_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    state.registers[rx] ^= state.registers[ry];
    set_flags_z_n(state, state.registers[rx]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn xor_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("xor_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    state.registers[rz] = state.registers[rx] ^ state.registers[ry];
    set_flags_z_n(state, state.registers[rz]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rz]);
    Ok(())
}
fn muli_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("muli_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = hhll(instruction) as i16;
    instr_dbg_println!("Multiplying {} * {}", state.registers[rx], val);
    state.registers[rx] = op_mul(state, state.registers[rx], val);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn mul_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("mul_rx_ry");
    let (rx, ry) = rx_ry(instruction);

    state.registers[rx] = op_mul(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn mul_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("mul_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    state.registers[rz] = op_mul(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rz, state.registers[rz]);
    Ok(())
}
fn divi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("divi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = hhll(instruction) as i16;
    instr_dbg_println!("Dividing {} / {}", state.registers[rx], val);
    state.registers[rx] = op_div(state, state.registers[rx], val);
    instr_dbg_println!("{:?}", state.flags);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn div_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("div_rx_ry");
    let (rx, ry) = rx_ry(instruction);

    state.registers[rx] = op_div(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn div_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("div_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    state.registers[rz] = op_div(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rz, state.registers[rz]);
    Ok(())
}
fn modi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("modi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = hhll(instruction) as i16;
    instr_dbg_println!("{} mod {}", state.registers[rx], val);
    state.registers[rx] = op_mod(state, state.registers[rx], val);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn mod_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("mod_rx_ry");
    let (rx, ry) = rx_ry(instruction);

    instr_dbg_println!("{} mod {}", state.registers[rx], state.registers[ry]);
    state.registers[rx] = op_mod(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn mod_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("mod_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    instr_dbg_println!("{} mod {}", state.registers[rx], state.registers[ry]);
    state.registers[rz] = op_mod(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rz, state.registers[rz]);
    Ok(())
}
fn remi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("remi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = hhll(instruction) as i16;
    state.registers[rx] = op_rem(state, state.registers[rx], val);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn rem_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("rem_rx_ry");
    let (rx, ry) = rx_ry(instruction);

    state.registers[rx] = op_rem(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn rem_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("rem_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    state.registers[rz] = op_rem(state, state.registers[rx], state.registers[ry]);
    instr_dbg_println!("Set register {:#02X?} to {:#02X?}", rz, state.registers[rz]);
    Ok(())
}
fn shl_rx_n(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("shl_rx_n");
    let rx = rx(instruction);
    let n = n(instruction);
    state.registers[rx] = state.registers[rx] << n;
    set_flags_z_n(state, state.registers[rx]);
    Ok(())
}
fn shr_rx_n(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("shr_rx_n");
    let rx = rx(instruction);
    let n = n(instruction);
    state.registers[rx] = (state.registers[rx] as u16 >> n) as i16;
    set_flags_z_n(state, state.registers[rx]);
    Ok(())
}
fn sar_rx_n(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("shr_rx_n");
    let rx = rx(instruction);
    let n = n(instruction);
    state.registers[rx] = state.registers[rx] >> n;
    set_flags_z_n(state, state.registers[rx]);
    Ok(())
}
fn shl_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("shl_rx_ry");
    let (rx, ry) = rx_ry(instruction);

    state.registers[rx] = state.registers[rx] << state.registers[ry];
    set_flags_z_n(state, state.registers[rx]);
    Ok(())
}
fn shr_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("shr_rx_ry");
    let (rx, ry) = rx_ry(instruction);

    state.registers[rx] = (state.registers[rx] as u16 >> state.registers[ry]) as i16;
    set_flags_z_n(state, state.registers[rx]);

    Ok(())
}
fn sar_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("sar_rx_ry");
    let (rx, ry) = rx_ry(instruction);

    state.registers[rx] = state.registers[rx] >> state.registers[ry];
    set_flags_z_n(state, state.registers[rx]);
    Ok(())
}
fn push_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("push_rx");
    let rx = rx(instruction);
    push_reg(state, rx);
    Ok(())
}
fn pop_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("pop_rx");
    let rx = rx(instruction);
    pop_reg(state, rx);
    Ok(())
}
fn pushall(state: &mut CPU, _instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("pushall");
    for r in 0..(state.registers.len()) {
        push_reg(state, r);
    }
    Ok(())
}
fn popall(state: &mut CPU, _instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("popall");

    for r in 0..(state.registers.len()) {
        let reg = state.registers.len() - 1 - r;
        pop_reg(state, reg);
    }
    Ok(())
}
fn pushf(state: &mut CPU, _instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("pushf");
    let mut val = 0;
    if state.flags.C {
        val |= 0b00000010;
    }
    if state.flags.Z {
        val |= 0b00000100;
    }
    if state.flags.O {
        val |= 0b01000000;
    }
    if state.flags.N {
        val |= 0b10000000;
    }
    state.mem[state.sp] = val;

    state.sp += 2;
    Ok(())
}
fn popf(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("popf");
    state.sp -= 2;
    let val = state.mem[state.sp];

    state.flags.C = 0b00000010 & val > 0;
    state.flags.Z = 0b00000100 & val > 0;
    state.flags.O = 0b01000000 & val > 0;
    state.flags.N = 0b10000000 & val > 0;

    Ok(())
}
fn pal_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("pal_hhll");
    let addr = hhll(instruction);
    instr_dbg_println!("Loading palette from {:X}", addr);
    load_palette(state, addr as usize);
    Ok(())
}
fn pal_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("pal_rx");
    let addr = (state.registers[rx(instruction)]) as usize & 0xFFFF;
    instr_dbg_println!("Loading palette from {:02X}", addr);
    load_palette(state, addr as usize);
    Ok(())
}
fn noti_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("noti_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    state.registers[rx] = op_not(state, hhll(instruction) as i16);
    Ok(())
}
fn not_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("not_rx");
    let (rx, _) = rx_ry(instruction);
    state.registers[rx] = op_not(state, state.registers[rx]);
    Ok(())
}
fn not_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("not_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    state.registers[rx] = op_not(state, state.registers[ry]);
    Ok(())
}
fn negi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("negi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    state.registers[rx] = op_neg(state, hhll(instruction) as i16);
    Ok(())
}

fn neg_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("neg_rx");
    let (rx, _) = rx_ry(instruction);
    state.registers[rx] = op_neg(state, state.registers[rx]);
    Ok(())
}
fn neg_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    instr_dbg_println!("neg_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    state.registers[rx] = op_neg(state, state.registers[ry]);
    Ok(())
}

fn test_cond(state: &mut CPU, instruction: &Instruction) -> Result<bool, String> {
    let cond = instruction[1] & 0xF;
    instr_dbg_println!("Testing cond {:X}", cond);

    match cond {
        0x0 => Ok(state.flags.Z),                                    // Z == 1
        0x1 => Ok(!state.flags.Z),                                   // Z == 0
        0x2 => Ok(state.flags.N),                                    // N == 1
        0x3 => Ok(!state.flags.N),                                   // N == 0
        0x4 => Ok(!state.flags.Z && !state.flags.N),                 // Z == 0 && N == 0
        0x5 => Ok(state.flags.O),                                    // O == 1
        0x6 => Ok(!state.flags.O),                                   // O == 0
        0x7 => Ok(!state.flags.C && !state.flags.Z),                 // C == 0 && Z == 0
        0x8 => Ok(!state.flags.C),                                   // C == 0
        0x9 => Ok(state.flags.C),                                    // C == 1
        0xA => Ok(state.flags.C || state.flags.Z),                   // C > 0 || Z > 0
        0xB => Ok(state.flags.O == state.flags.N && !state.flags.Z), // O == N && Z == 0
        0xC => Ok(state.flags.O == state.flags.N),                   // O == N
        0xD => Ok(state.flags.O != state.flags.N),                   // O != N
        0xE => Ok(state.flags.O != state.flags.N || state.flags.Z),  // O != N || Z == 1
        0xF => Err(String::from("Invalid condition")),
        _ => Err(String::from("Invalid condition")),
    }
}

fn op_add(state: &mut CPU, val1: i16, val2: i16) -> i16 {
    let result = ((val1 as u32) & 0xFFFF) + ((val2 as u32) & 0xFFFF);
    state.flags.C = result > u16::MAX as u32;
    // Bitwise & to handle overflow having made the 16 bits 0
    state.flags.Z = (result & 0xFFFF) == 0;
    state.flags.O = (result as i32) < i32::from(i16::MIN) || (result as i32) > i32::from(i16::MAX);
    state.flags.N = ((result & 0xFFFF) as i16) < 0;

    return (result & 0xFFFF) as i16;
}

fn op_sub(state: &mut CPU, val1: i16, val2: i16) -> i16 {
    let result = i32::from(val1) - i32::from(val2);
    state.flags.C = (val1 as u16) < (val2 as u16);
    state.flags.Z = result == 0;
    state.flags.O = result < i32::from(i16::MIN) || result > i32::from(i16::MAX);
    state.flags.N = result < 0;

    return (result & 0xFFFF) as i16;
}

fn and_flags(state: &mut CPU, val1: i16, val2: i16) {
    let result = val1 & val2;
    state.flags.Z = result == 0;
    state.flags.N = result < 0;
}

fn set_flags_z_n(state: &mut CPU, val: i16) {
    state.flags.Z = val == 0;
    state.flags.N = val < 0;
}

fn op_mul(state: &mut CPU, val1: i16, val2: i16) -> i16 {
    let mut result = ((val1 as u32) & 0xFFFF) * ((val2 as u32) & 0xFFFF);
    state.flags.C = result > (u16::MAX as u32);

    result &= 0xFFFF;

    state.flags.Z = result == 0;
    state.flags.N = (result as i16) < 0;

    return result as i16;
}

fn op_div(state: &mut CPU, val1: i16, val2: i16) -> i16 {
    let result = val1 / val2;
    state.flags.C = val1 % val2 != 0;
    state.flags.Z = result == 0;
    state.flags.N = (result as i16) < 0;

    return result as i16;
}

fn op_mod(state: &mut CPU, val1: i16, val2: i16) -> i16 {
    let v1 = i32::from(val1);
    let v2 = i32::from(val2);

    // rust implementes the remainder operation with the % operator, we want modulo.
    // From https://stackoverflow.com/questions/31210357/is-there-a-modulus-not-remainder-function-operation
    let result = ((v1 % v2) + v2) % v2;

    state.flags.Z = result == 0;
    state.flags.N = result < 0;

    instr_dbg_println!("Mod result: {}", result);
    return (result & 0xFFFF) as i16;
}

fn op_rem(state: &mut CPU, val1: i16, val2: i16) -> i16 {
    let result = i32::from(val1) % i32::from(val2);

    state.flags.Z = result == 0;
    state.flags.N = result < 0;

    return (result & 0xFFFF) as i16;
}

fn op_not(state: &mut CPU, val1: i16) -> i16 {
    let result = !val1;
    state.flags.Z = result == 0;
    state.flags.N = result < 0;

    return result;
}

fn op_neg(state: &mut CPU, val1: i16) -> i16 {
    let result = -1 * val1;
    state.flags.Z = result == 0;
    state.flags.N = result < 0;

    return result;
}

fn push_reg(state: &mut CPU, register: usize) {
    instr_dbg_println!(
        "Pushing r{:X}({}) to stack",
        register,
        state.registers[register]
    );

    store_mem(state, state.registers[register] as u16, state.sp);
    state.sp += 2;
}

fn pop_reg(state: &mut CPU, register: usize) {
    state.sp -= 2;
    let addr = state.sp;
    state.registers[register] = load_mem(state, addr) as i16;
    instr_dbg_println!(
        "Popped r{:X}({}) from stack",
        register,
        state.registers[register]
    );
}
fn load_mem(state: &mut CPU, addr: usize) -> u16 {
    let a = addr & 0xFFFF;
    return (&state.mem[a..(a + 2)])
        .read_u16::<LE>()
        .expect("Failed to read from cpu state mem");
}

fn store_mem(state: &mut CPU, val: u16, addr: usize) {
    let a = addr & 0xFFFF;
    (&mut state.mem[a..(a + 2)])
        .write_u16::<LE>(val)
        .expect("Failed to write to mem");

    if (load_mem(state, a) != val) {
        println!("Failed at writing with write_u16");
    }
    instr_dbg_println!(
        "Set [{:#02X?}] to {:#02X?}, {:#02X?}",
        addr,
        state.mem[a],
        state.mem[a + 1]
    );
}

fn load_palette(state: &mut CPU, start_addr: usize) {
    for idx in 0..16 {
        let addr = start_addr + idx * 3;
        state.palette[idx] = ((state.mem[addr] as u32) << 16)
            + ((state.mem[addr + 1] as u32) << 8)
            + state.mem[addr + 2] as u32;
    }
    instr_dbg_println!("{:X?}", state.palette);
}

fn in_bounds(x: i16, y: i16) -> bool {
    return (x >= 0 && x < SCREEN_SIZE_X as i16) && (y >= 0 && y < SCREEN_SIZE_Y as i16);
}

fn draw_sprite(state: &mut CPU, x_coord: i16, y_coord: i16, sprite_addr: u16) {
    //dbg_println!(
    //    "Draw sprite from {:#02X?} at {}, {}",
    //    sprite_addr,
    //    x_coord,
    //    y_coord
    //);

    let y_range = std::ops::Range {
        start: 0,
        end: state.graphics.spriteh,
    };

    let mut intersected = 0u32;

    for y in y_range {
        // For vflip mirror which sprite address we get, height - 1 as the range does not include the end value
        let y_mem = if state.graphics.vflip {
            state.graphics.spriteh - 1 - y
        } else {
            y
        };

        let x_range = std::ops::Range {
            start: 0,
            end: state.graphics.spritew,
        };

        for x in x_range {
            // For hflip mirror which sprite address we get, width - 1 as the range does not include the end value
            let x_mem = if state.graphics.hflip {
                state.graphics.spritew - 1 - x
            } else {
                x
            };

            let cur_sprite_addr = usize::from(y_mem) * usize::from(state.graphics.spritew)
                + usize::from(sprite_addr)
                + usize::from(x_mem);
            let sprite_byte = state.mem[cur_sprite_addr];

            // For hflip we need to flip the pixels too, as we're mirroring across the y axis
            let (lpx, rpx) = if state.graphics.hflip {
                (sprite_byte & 0xF, sprite_byte >> 4)
            } else {
                (sprite_byte >> 4, sprite_byte & 0xF)
            };

            // Each byte is 2 pixels, so * 2 to get the correct coordinate in the screen
            let lx_pos = x_coord + i16::from(x) * 2;
            let rx_pos = lx_pos + 1;
            let y_pos = y_coord + i16::from(y);

            //dbg_println!(
            //    "Setting {}, {}: {:#02X?} from {:#02X?}",
            //    lx_pos,
            //    y_pos,
            //    lpx,
            //    cur_sprite_addr
            //);

            if lpx > 0 && in_bounds(lx_pos, y_pos) {
                let screen_idx = y_pos as usize * SCREEN_SIZE_X as usize + lx_pos as usize;
                intersected += state.screen[screen_idx] as u32;
                state.screen[screen_idx] = lpx;
            }

            //dbg_println!(
            //    "Setting {}, {}: {:#02X?} from {:#02X?}",
            //    rx_pos,
            //    y_pos,
            //    lpx,
            //    cur_sprite_addr
            //);
            if rpx > 0 && in_bounds(rx_pos, y_pos) {
                let screen_idx = y_pos as usize * SCREEN_SIZE_X as usize + rx_pos as usize;
                intersected += state.screen[screen_idx] as u32;
                state.screen[screen_idx] = rpx;
            }
        }
    }

    state.flags.C = intersected > 0;
}

#[derive(Debug)]
pub struct FLAGS {
    C: bool,
    Z: bool,
    O: bool,
    N: bool,
}

pub struct GPU {
    bg: u8,
    spritew: u8,
    spriteh: u8,
    hflip: bool,
    vflip: bool,
}

type Controller = u8;

pub struct CPU<'a> {
    ops: Vec<for<'b, 'c> fn(&'b mut CPU, &'c Instruction) -> Result<(), String>>,
    registers: [i16; 16],
    pc: u16,
    sp: usize,
    flags: FLAGS,
    vblnk: bool,
    mem: [u8; 65536],
    graphics: GPU,
    screen: [u8; SCREEN_BUF_SIZE],
    palette: [u32; 16],
    controls: [Controller; 2],
    cycles: u32,
    event_pump: &'a mut EventPump,
    audio_state: &'a mut audio::AudioState<'a>,
    stack: Vec<u16>,
}

impl CPU<'_> {
    pub fn new<'a>(
        mem: &'a mut [u8; 65536],
        event_pump: &'a mut EventPump,
        audio_state: &'a mut AudioState<'a>,
    ) -> CPU<'a> {
        return CPU {
            ops: vec![],
            registers: [0x00; 16],
            pc: 0x00,
            sp: 0xFDF0,
            flags: FLAGS {
                N: false,
                O: false,
                Z: false,
                C: false,
            },
            vblnk: false,
            mem: mem.clone(),
            graphics: GPU {
                bg: 0x0,
                spritew: 0,
                spriteh: 0,
                hflip: false,
                vflip: false,
            },
            controls: [0, 0],
            screen: [0x00; SCREEN_BUF_SIZE],
            palette: [
                0x000000, 0x000000, 0x888888, 0xBF3232, 0xDE7AAE, 0x4C3D21, 0x905F25, 0xE49452,
                0xEAD979, 0x537A3B, 0xABD54A, 0x252E38, 0x00467F, 0x68ABCC, 0xBCDEE4, 0xFFFFFF,
            ],
            cycles: 0,
            event_pump,
            audio_state,
            stack: vec![],
        };
    }
    pub fn init(&mut self) {
        for _ in 0..0xFF {
            self.ops.push(error);
        }
        self.ops[0x00] = nop;
        self.ops[0x01] = cls;
        self.ops[0x02] = vblnk;
        self.ops[0x03] = bgc_n;
        self.ops[0x04] = spr_hhll;
        self.ops[0x05] = drw_rx_ry_hhll;
        self.ops[0x06] = drw_rx_ry_rz;
        self.ops[0x07] = rnd_rx_hhll;
        self.ops[0x08] = flip;
        self.ops[0x09] = snd0;
        self.ops[0x0a] = snd1_hhll;
        self.ops[0x0b] = snd2_hhll;
        self.ops[0x0c] = snd3_hhll;
        self.ops[0x0d] = snp_rx_hhll;
        self.ops[0x0e] = sng_ad_vtsr;

        self.ops[0x10] = jmp_hhll;
        self.ops[0x11] = jmc_hhll;
        self.ops[0x12] = jx_hhll;
        self.ops[0x13] = jme_rx_ry_hhll;
        self.ops[0x14] = call_hhll;
        self.ops[0x15] = ret;
        self.ops[0x16] = jmp_rx;
        self.ops[0x17] = cx_hhll;
        self.ops[0x18] = call_rx;

        self.ops[0x20] = ldi_rx_hhll;
        self.ops[0x21] = ldi_sp_hhll;
        self.ops[0x22] = ldm_rx_hhll;
        self.ops[0x23] = ldm_rx_ry;
        self.ops[0x24] = mov_rx_ry;

        self.ops[0x30] = stm_rx_hhll;
        self.ops[0x31] = stm_rx_ry;

        self.ops[0x40] = addi_rx_hhll;
        self.ops[0x41] = add_rx_ry;
        self.ops[0x42] = add_rx_ry_rz;

        self.ops[0x50] = subi_rx_hhll;
        self.ops[0x51] = sub_rx_ry;
        self.ops[0x52] = sub_rx_ry_rz;
        self.ops[0x53] = cmpi_rx_hhll;
        self.ops[0x54] = cmp_rx_ry;

        self.ops[0x60] = andi_rx_hhll;
        self.ops[0x61] = and_rx_ry;
        self.ops[0x62] = and_rx_ry_rz;
        self.ops[0x63] = tsti_rx_hhll;
        self.ops[0x64] = tst_rx_ry;

        self.ops[0x70] = ori_rx_hhll;
        self.ops[0x71] = or_rx_ry;
        self.ops[0x72] = or_rx_ry_rz;

        self.ops[0x80] = xori_rx_hhll;
        self.ops[0x81] = xor_rx_ry;
        self.ops[0x82] = xor_rx_ry_rz;

        self.ops[0x90] = muli_rx_hhll;
        self.ops[0x91] = mul_rx_ry;
        self.ops[0x92] = mul_rx_ry_rz;

        self.ops[0xA0] = divi_rx_hhll;
        self.ops[0xA1] = div_rx_ry;
        self.ops[0xA2] = div_rx_ry_rz;
        self.ops[0xA3] = modi_rx_hhll;
        self.ops[0xA4] = mod_rx_ry;
        self.ops[0xA5] = mod_rx_ry_rz;
        self.ops[0xA6] = remi_rx_hhll;
        self.ops[0xA7] = rem_rx_ry;
        self.ops[0xA8] = rem_rx_ry_rz;

        self.ops[0xB0] = shl_rx_n;
        self.ops[0xB1] = shr_rx_n;
        self.ops[0xB2] = sar_rx_n;
        self.ops[0xB3] = shl_rx_ry;
        self.ops[0xB4] = shr_rx_ry;
        self.ops[0xB5] = sar_rx_ry;

        self.ops[0xC0] = push_rx;
        self.ops[0xC1] = pop_rx;
        self.ops[0xC2] = pushall;
        self.ops[0xC3] = popall;
        self.ops[0xC4] = pushf;
        self.ops[0xC5] = popf;

        self.ops[0xD0] = pal_hhll;
        self.ops[0xD1] = pal_rx;

        self.ops[0xE0] = noti_rx_hhll;
        self.ops[0xE1] = not_rx;
        self.ops[0xE2] = not_rx_ry;
        self.ops[0xE3] = negi_rx_hhll;
        self.ops[0xE4] = neg_rx;
        self.ops[0xE5] = neg_rx_ry;
    }

    fn execute(&mut self, instruction: &Instruction) {
        let op = self.ops[usize::from(instruction[0])];
        op(self, instruction).expect("Failed to run instruction");
    }

    pub fn set_pc(&mut self, address: [u8; 2]) {
        self.pc = (address[1] as u16) << 8 + address[0] as u16;
    }

    pub fn run(&mut self, renderer: &mut Renderer) -> Result<(), String> {
        self.stack.push(self.pc);

        let mut previous_frame_time = Instant::now();
        'running: loop {
            let pc = usize::from(self.pc);
            let next_inst = [
                self.mem[pc],
                self.mem[pc + 1],
                self.mem[pc + 2],
                self.mem[pc + 3],
            ];

            if (next_inst[0] != 0x10
                || (((next_inst[3] as u16) << 8) | next_inst[2] as u16) != self.pc)
                && next_inst[0] != 0x2
            {
                instr_dbg_println!(
                    "{:X}: {:X} {:X} {:X} {:X}",
                    self.pc,
                    next_inst[0],
                    next_inst[1],
                    next_inst[2],
                    next_inst[3]
                );
                instr_dbg_println!("{:X?}", self.stack);
            }

            self.pc += 4;
            if let Some(cur_stack) = self.stack.last_mut() {
                *cur_stack = self.pc;
            }

            self.execute(&next_inst);

            self.cycles += 1;

            //dbg_println!("{}", cycles);
            if self.cycles >= FRAME_CYCLES {
                self.cycles = 0;
                //dbg_println!("Sleeping and drawing");
                renderer.draw(self)?;
                //self.audio.play((1000 / 60) * 2, self.audio_file)?;

                if self.audio_state.is_finished() {
                    self.audio_state.clear();
                    //println!("Finished playing sound");
                }
                let passed_duration = previous_frame_time.elapsed();
                //println!(
                //    "Time elapsed since last frame draw is: {:?}, frame duration: {:?}",
                //    passed_duration, FRAME_DURATION
                //);
                if passed_duration < FRAME_DURATION {
                    let sleep_duration = FRAME_DURATION - passed_duration;
                    //dbg_println!("Sleeping for {:?}", sleep_duration);

                    thread::sleep(sleep_duration);
                }
                self.vblnk = true;

                let mut up_events: Vec<Keycode> = vec![];
                for event in self.event_pump.poll_iter() {
                    match event {
                        Event::Quit { .. } => break 'running,
                        Event::KeyDown {
                            keycode: Some(keycode),
                            ..
                        } => match keycode {
                            Keycode::W => self.controls[0] |= 0b00000001,
                            Keycode::S => self.controls[0] |= 0b00000010,
                            Keycode::A => self.controls[0] |= 0b00000100,
                            Keycode::D => self.controls[0] |= 0b00001000,
                            Keycode::G => self.controls[0] |= 0b00010000,
                            Keycode::H => self.controls[0] |= 0b00100000,
                            Keycode::J => self.controls[0] |= 0b01000000,
                            Keycode::K => self.controls[0] |= 0b10000000,
                            _ => {}
                        },
                        Event::KeyUp {
                            keycode: Some(keycode),
                            ..
                        } => up_events.push(keycode),
                        _ => {}
                    }
                }

                self.update_control_mem();

                for keycode in up_events {
                    match keycode {
                        Keycode::W => self.controls[0] &= 0b11111110,
                        Keycode::S => self.controls[0] &= 0b11111101,
                        Keycode::A => self.controls[0] &= 0b11111011,
                        Keycode::D => self.controls[0] &= 0b11110111,
                        Keycode::G => self.controls[0] &= 0b11101111,
                        Keycode::H => self.controls[0] &= 0b11011111,
                        Keycode::J => self.controls[0] &= 0b10111111,
                        Keycode::K => self.controls[0] &= 0b01111111,
                        _ => {}
                    }
                }

                previous_frame_time += FRAME_DURATION;
            } else {
                self.vblnk = false;
            }
        }
        Ok(())
    }

    pub fn screen(&mut self) -> [u8; SCREEN_BUF_SIZE] {
        return self.screen;
    }

    pub fn palette(&mut self) -> [u32; 16] {
        return self.palette;
    }

    pub fn bgc(&mut self) -> u8 {
        return self.graphics.bg;
    }

    fn update_control_mem(&mut self) {
        self.mem[0xFFF0] = self.controls[0];
        self.mem[0xFFF2] = self.controls[1];
    }
}
