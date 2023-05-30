use std::fs::File;
use std::thread;
use std::time::{Duration, Instant};

use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::EventPump;

use crate::audio::AudioState;
use crate::renderer::Renderer;
use crate::{audio, FRAME_CYCLES};

const SCREEN_SIZE_X: u16 = 320;
const SCREEN_SIZE_Y: u16 = 240;
const SCREEN_BUF_SIZE: usize = SCREEN_SIZE_X as usize * SCREEN_SIZE_Y as usize;
const FRAME_DURATION: Duration = Duration::new(0, 1_000_000_000u32 / 60);

type Instruction = [u8; 4];

fn hhll(instruction: &Instruction) -> u16 {
    return (u16::from(instruction[3]) << 8) + u16::from(instruction[2]);
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
        usize::from(instruction[2 & 0xF]),
    );
}

fn nop(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("noop");
    Ok(())
}

fn error(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("Invalid command {:#02X?}", instruction[0]);
    Err(String::from("Invalid command"))
}

fn cls(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("cls");
    state.graphics.bg = 0;
    state.screen.iter_mut().for_each(|m| *m = 0);
    Ok(())
}

fn vblnk(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("vblnk");
    // Skip to frame render if we hit vblank
    if !state.vblnk {
        state.cycles = FRAME_CYCLES
    }
    Ok(())
}

fn bgc_n(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("bgc_n");
    state.graphics.bg = instruction[2] & 0xF;
    dbg_println!("Set bg to {:#02X?}", state.graphics.bg);
    Ok(())
}

fn spr_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("spr_hhll");
    state.graphics.spritew = instruction[2];
    state.graphics.spriteh = instruction[3];
    dbg_println!(
        "Set spriteh: {:#02X?}, spritew: {:#02X?}",
        state.graphics.spriteh,
        state.graphics.spritew
    );

    Ok(())
}

fn drw_rx_ry_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    let (rx, ry) = rx_ry(instruction);
    let sprite_addr = hhll(instruction);

    dbg_println!("drw_r{:#02X}_r{:#02X}_{:#04X}", rx, ry, sprite_addr);
    draw_sprite(state, state.registers[rx], state.registers[ry], sprite_addr);
    Ok(())
}

fn drw_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("drw_rx_ry_rz");
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
    Err(String::from("rnd_rx_hhll"))
}
fn flip(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("flip"))
}
fn snd0(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    //println!("snd0");
    state.audio_state.clear();
    Ok(())
}
fn snd1_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    //println!("snd1_hhll");
    dbg_println!("Playing for {} ms", hhll(instruction));

    state.audio_state.set_params(500, hhll(instruction));
    state.audio_state.start();

    Ok(())
}
fn snd2_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    //println!("snd2_hhll");
    state.audio_state.set_params(1000, hhll(instruction));
    state.audio_state.start();

    Ok(())
}
fn snd3_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    // println!("snd3_hhll");
    state.audio_state.set_params(1500, hhll(instruction));
    state.audio_state.start();
    Ok(())
}

fn snp_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    // println!("snp_rx_hhll");
    let rx = rx(instruction);
    state
        .audio_state
        .set_params(state.registers[rx] as u16, hhll(instruction));

    state.audio_state.start();
    Ok(())
}

fn sng_ad_vtsr(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("sng_ad_vtsr"))
}

fn jmp_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    //dbg_println!("jmp_hhll");
    state.pc = hhll(instruction);
    //dbg_println!("Set pc to {:#02X?}", state.pc);

    Ok(())
}

fn jmc_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("jmc_hhll"))
}

fn jx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!(
        "j{:#02X?}_{:#04X?}",
        instruction[1] & 0xF,
        hhll(instruction)
    );
    if test_cond(state, instruction)? {
        state.pc = hhll(instruction);
        dbg_println!("Set pc to {:#02X?}", state.pc);
    }
    Ok(())
}

fn jme_rx_ry_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    let (rx, ry) = rx_ry(instruction);

    dbg_println!("jme_r{:#02X}_r{:#02X}_{:#04X?}", rx, ry, hhll(instruction));

    if state.registers[rx] == state.registers[ry] {
        state.pc = hhll(instruction);
    }
    Ok(())
}

fn call_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("call_hhll");
    state.mem[state.sp as usize] = (state.pc >> 8) as u8;
    state.mem[(state.sp + 1) as usize] = (state.pc & 0xFF) as u8;
    state.sp += 2;
    state.pc = hhll(instruction);
    dbg_println!("Set pc to {:#02X?}", state.pc);
    Ok(())
}
fn ret(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("ret");
    state.sp -= 2;
    let addr = state.sp as usize;
    state.pc = ((state.mem[addr] as u16) << 8) + state.mem[addr + 1] as u16;

    Ok(())
}
fn jmp_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("jmp_rx");
    let (rx, _) = rx_ry(instruction);
    state.pc = state.registers[rx] as u16;
    Ok(())
}
fn cx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("cx_hhll"))
}
fn call_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("call_rx"))
}
fn ldi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("ldi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    state.registers[rx] = hhll(instruction) as i16;
    dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn ldi_sp_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("ldi_sp_hhll"))
}

fn load_mem(state: &mut CPU, addr: usize) -> i16 {
    return ((u16::from(state.mem[addr + 1]) << 8) + u16::from(state.mem[addr])) as i16;
}

fn ldm_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("ldm_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    state.registers[rx] = load_mem(state, hhll(instruction) as usize);

    dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}

fn ldm_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("ldm_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    state.registers[rx] = load_mem(state, state.registers[ry] as usize);

    dbg_println!(
        "Set register {:#02X?} to [register {:#02X?}] ([{:#02X?}]): {:#02X?}",
        rx,
        ry,
        state.registers[ry],
        state.registers[rx]
    );
    Ok(())
}
fn mov_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("mov_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    state.registers[rx] = state.registers[ry];
    dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}

fn store_mem(state: &mut CPU, val: u16, addr: usize) {
    state.mem[addr + 1] = (val >> 8) as u8;
    state.mem[addr] = (val & 0xFF) as u8;

    dbg_println!(
        "Set [{:#02X?}] to {:#02X?}, {:#02X?}",
        addr,
        state.mem[addr],
        state.mem[addr + 1]
    );
}
fn stm_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("stm_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let addr = hhll(instruction) as usize;
    store_mem(state, state.registers[rx] as u16, addr);
    Ok(())
}

fn stm_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("stm_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    store_mem(
        state,
        state.registers[rx] as u16,
        state.registers[ry] as usize,
    );
    Ok(())
}

fn addi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("addi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = hhll(instruction) as i16;
    state.registers[rx] = op_add(state, state.registers[rx], val);
    dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn add_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("add_rx_ry");
    let (rx, ry) = rx_ry(instruction);

    state.registers[rx] = op_add(state, state.registers[rx], state.registers[ry]);
    dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn add_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("add_rx_ry_rz"))
}
fn subi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("subi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = ((u16::from(instruction[3]) << 0x8) + u16::from(instruction[2])) as i16;

    state.registers[rx] = op_sub(state, state.registers[rx], val);
    dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn sub_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("sub_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    state.registers[rx] = op_sub(state, state.registers[rx], state.registers[ry]);
    dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);
    Ok(())
}
fn sub_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("sub_rx_ry_rz");
    let (rx, ry, rz) = rx_ry_rz(instruction);
    state.registers[rz] = op_sub(state, state.registers[rx], state.registers[ry]);
    dbg_println!("Set register {:#02X?} to {:#02X?}", rz, state.registers[rz]);

    Ok(())
}
fn cmpi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("cmpi_rx_hhll"))
}
fn cmp_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("cmp_rx_ry"))
}
fn andi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("andi_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = ((u16::from(instruction[3]) << 0x8) + u16::from(instruction[2])) as i16;
    and_flags(state, state.registers[rx], val);
    state.registers[rx] &= val;
    dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn and_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("and_rx_ry");
    let (rx, ry) = rx_ry(instruction);
    and_flags(state, state.registers[rx], state.registers[ry]);
    state.registers[rx] &= state.registers[ry];
    dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn and_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("and_rx_ry_rz"))
}
fn tsti_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("tsti_rx_hhll"))
}
fn tst_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("tst_rx_ry"))
}
fn ori_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("ori_rx_hhll"))
}
fn or_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("or_rx_ry"))
}
fn or_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("or_rx_ry_rz"))
}
fn xori_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("xori_rx_hhll"))
}
fn xor_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("xor_rx_ry"))
}
fn xor_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("xor_rx_ry_rz"))
}
fn muli_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    dbg_println!("muli_rx_hhll");
    let (rx, _) = rx_ry(instruction);
    let val = ((u16::from(instruction[3]) << 0x8) + u16::from(instruction[2])) as i16;
    state.registers[rx] = op_mul(state, state.registers[rx], val);
    dbg_println!("Set register {:#02X?} to {:#02X?}", rx, state.registers[rx]);

    Ok(())
}
fn mul_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("mul_rx_ry"))
}
fn mul_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("mul_rx_ry_rz"))
}
fn divi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("divi_rx_hhll"))
}
fn div_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("div_rx_ry"))
}
fn div_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("div_rx_ry_rz"))
}
fn modi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("modi_rx_hhll"))
}
fn mod_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("mod_rx_ry"))
}
fn mod_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("mod_rx_ry_rz"))
}
fn remi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("remi_rx_hhll"))
}
fn rem_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("rem_rx_ry"))
}
fn rem_rx_ry_rz(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("rem_rx_ry_rz"))
}
fn shl_rx_n(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("shl_rx_n"))
}
fn shr_rx_n(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("shr_rx_n"))
}
fn sar_rx_n(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("sar_rx_n"))
}
fn shl_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("shl_rx_ry"))
}
fn shr_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("shr_rx_ry"))
}
fn sar_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("sar_rx_ry"))
}
fn push_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("push_rx"))
}
fn pop_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("pop_rx"))
}
fn pushall(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("pushall"))
}
fn popall(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("popall"))
}
fn pushf(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("pushf"))
}
fn popf(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("popf"))
}
fn pal_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("pal_hhll"))
}
fn pal_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("pal_rx"))
}
fn noti_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("noti_rx_hhll"))
}
fn not_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("not_rx"))
}
fn not_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("not_rx_ry"))
}
fn negi_rx_hhll(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("negi_rx_hhll"))
}

fn neg_rx(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("neg_rx"))
}
fn neg_rx_ry(state: &mut CPU, instruction: &Instruction) -> Result<(), String> {
    Err(String::from("neg_rx_ry"))
}

fn test_cond(state: &mut CPU, instruction: &Instruction) -> Result<bool, String> {
    let cond = instruction[1] & 0xF;

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
    let result = i32::from(val1) + i32::from(val2);
    state.flags.C = result < i32::from(i16::MIN) || result > i32::from(i16::MAX);
    // Bitwise & to handle overflow having made the 16 bits 0
    state.flags.Z = (result & 0xFFFF) == 0;
    state.flags.O = result < i32::from(i16::MIN) || result > i32::from(i16::MAX);
    state.flags.N = result < 0;

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
    state.flags.C = false;
    state.flags.O = false;
    state.flags.Z = result == 0;
    state.flags.N = result < 0;
}

fn op_mul(state: &mut CPU, val1: i16, val2: i16) -> i16 {
    let result = i32::from(val1) * i32::from(val2);
    state.flags.O = result < i32::from(i16::MIN) || result > i32::from(i16::MAX);
    state.flags.C = result < i32::from(i16::MIN) || result > i32::from(i16::MAX);
    state.flags.Z = result == 0;
    state.flags.N = result < 0;

    return (result & 0xFFFF) as i16;
}

fn in_bounds(x: i16, y: i16) -> bool {
    return (x >= 0 && x < SCREEN_SIZE_X as i16) && (y >= 0 && y < SCREEN_SIZE_Y as i16);
}

fn draw_sprite(state: &mut CPU, x_coord: i16, y_coord: i16, sprite_addr: u16) {
    dbg_println!(
        "Draw sprite from {:#02X?} at {}, {}",
        sprite_addr,
        x_coord,
        y_coord
    );

    let y_range = std::ops::Range {
        start: 0,
        end: state.graphics.spriteh,
    };

    let mut intersected = 0;

    for y in y_range {
        let x_range = std::ops::Range {
            start: 0,
            end: state.graphics.spritew,
        };

        for x in x_range {
            let cur_sprite_addr = usize::from(y) * usize::from(state.graphics.spritew)
                + usize::from(sprite_addr)
                + usize::from(x);
            let sprite_byte = state.mem[cur_sprite_addr];

            let lpx = sprite_byte >> 4;
            let rpx = sprite_byte & 0xF;
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

            if in_bounds(lx_pos, y_pos) {
                let screen_idx = y_pos as usize * SCREEN_SIZE_X as usize + lx_pos as usize;
                intersected += screen_idx;
                state.screen[screen_idx] = lpx;
            }

            //dbg_println!(
            //    "Setting {}, {}: {:#02X?} from {:#02X?}",
            //    rx_pos,
            //    y_pos,
            //    lpx,
            //    cur_sprite_addr
            //);
            if in_bounds(rx_pos, y_pos) {
                let screen_idx = y_pos as usize * SCREEN_SIZE_X as usize + rx_pos as usize;
                intersected += screen_idx;
                state.screen[screen_idx] = rpx;
            }
        }
    }

    state.flags.C = intersected > 0;
}

fn print_screen(screen: &[u8; SCREEN_BUF_SIZE]) {
    let y_range = std::ops::Range {
        start: 0,
        end: SCREEN_SIZE_Y,
    };

    for y in y_range {
        let x_range = std::ops::Range {
            start: 0,
            end: SCREEN_SIZE_X,
        };

        for x in x_range {
            //dbg_println!("{}", y * SCREEN_SIZE_Y + x);
            print!("{} ", screen[(y * SCREEN_SIZE_Y + x) as usize]);
        }
        print!("\n");
    }
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
    sp: u16,
    flags: FLAGS,
    vblnk: bool,
    mem: [u8; 65536],
    graphics: GPU,
    screen: [u8; SCREEN_BUF_SIZE],
    controls: [Controller; 2],
    cycles: u32,
    event_pump: &'a mut EventPump,
    audio_state: &'a mut audio::AudioState<'a>,
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
            cycles: 0,
            event_pump,
            audio_state,
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

        self.ops[0xC0] = noti_rx_hhll;
        self.ops[0xC1] = not_rx;
        self.ops[0xC2] = not_rx_ry;
        self.ops[0xC3] = negi_rx_hhll;
        self.ops[0xC4] = neg_rx;
        self.ops[0xC5] = neg_rx_ry;
    }

    fn execute(&mut self, instruction: &Instruction) {
        let op = self.ops[usize::from(instruction[0])];
        op(self, instruction).expect("Failed to run instruction");
    }

    pub fn set_pc(&mut self, address: [u8; 2]) {
        self.pc = (address[0] as u16) << 8 + address[1] as u16;
    }

    fn wipe_flags(&mut self) {
        self.flags.C = false;
        self.flags.Z = false;
        self.flags.O = false;
        self.flags.N = false;
    }

    pub fn run(&mut self, renderer: &mut Renderer) -> Result<(), String> {
        let mut previous_frame_time = Instant::now();
        'running: loop {
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
                    } => match keycode {
                        Keycode::W => self.controls[0] &= 0b11111110,
                        Keycode::S => self.controls[0] &= 0b11111101,
                        Keycode::A => self.controls[0] &= 0b11111011,
                        Keycode::D => self.controls[0] &= 0b11110111,
                        Keycode::G => self.controls[0] &= 0b11101111,
                        Keycode::H => self.controls[0] &= 0b11011111,
                        Keycode::J => self.controls[0] &= 0b10111111,
                        Keycode::K => self.controls[0] &= 0b01111111,
                        _ => {}
                    },

                    _ => {}
                }
            }

            let pc = usize::from(self.pc);
            let next_inst = [
                self.mem[pc],
                self.mem[pc + 1],
                self.mem[pc + 2],
                self.mem[pc + 3],
            ];

            //print!(
            //    "PC {:#02X?}: {:#02X?} {:#02X?} {:#02X?} {:#02X?}: ",
            //    self.pc, next_inst[0], next_inst[1], next_inst[2], next_inst[3]
            //);

            self.pc += 4;

            self.execute(&next_inst);

            self.cycles += 1;

            //dbg_println!("{}", cycles);
            if self.cycles >= FRAME_CYCLES {
                self.cycles = 0;
                dbg_println!("Sleeping and drawing");
                renderer.draw(self)?;
                //self.audio.play((1000 / 60) * 2, self.audio_file)?;

                if self.audio_state.is_finished() {
                    self.audio_state.clear();
                }
                let passed_duration = previous_frame_time.elapsed();
                //println!(
                //    "Time elapsed since last frame draw is: {:?}, frame duration: {:?}",
                //    passed_duration, FRAME_DURATION
                //);
                if passed_duration < FRAME_DURATION {
                    let sleep_duration = FRAME_DURATION - passed_duration;
                    dbg_println!("Sleeping for {:?}", sleep_duration);

                    thread::sleep(sleep_duration);
                }
                self.vblnk = true;
                self.update_control_mem();
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
        return [
            0x000000, 0x000000, 0x888888, 0xBF3232, 0xDE7AAE, 0x4C3D21, 0x905F25, 0xE49452,
            0xEAD979, 0x537A3B, 0xABD54A, 0x252E38, 0x00467F, 0x68ABCC, 0xBCDEE4, 0xFFFFFF,
        ];
    }

    pub fn bgc(&mut self) -> u8 {
        return self.graphics.bg;
    }

    fn update_control_mem(&mut self) {
        self.mem[0xFFF0] = self.controls[0];
        self.mem[0xFFF2] = self.controls[1];
    }
}
