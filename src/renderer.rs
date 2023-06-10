use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::WindowCanvas;
use sdl2::video::Window;
use sdl2::EventPump;

use crate::cpu::CPU;
use crate::{DOT_SIZE_IN_PXS, GRID_X_SIZE, GRID_Y_SIZE};

pub struct Renderer {
    canvas: WindowCanvas,
}

impl Renderer {
    pub fn new(window: Window) -> Result<Renderer, String> {
        let canvas = window.into_canvas().build().map_err(|e| e.to_string())?;
        Ok(Renderer { canvas })
    }
    fn draw_dot(&mut self, x: u32, y: u32) -> Result<(), String> {
        self.canvas.fill_rect(Rect::new(
            (x * DOT_SIZE_IN_PXS) as i32,
            (y * DOT_SIZE_IN_PXS) as i32,
            DOT_SIZE_IN_PXS,
            DOT_SIZE_IN_PXS,
        ))?;

        Ok(())
    }

    fn draw_foreground(&mut self, cpu: &mut CPU) -> Result<(), String> {
        let screen = cpu.screen();
        let y_range: std::ops::Range<u32> = std::ops::Range {
            start: 0,
            end: GRID_Y_SIZE as u32,
        };

        for y in y_range {
            let x_range: std::ops::Range<u32> = std::ops::Range {
                start: 0,
                end: GRID_X_SIZE as u32,
            };
            for x in x_range {
                let px = screen[(y * GRID_X_SIZE as u32 + x) as usize];
                // 0 is transparent
                if px > 0 {
                    let draw_color = self.get_draw_color(cpu, px);
                    self.canvas.set_draw_color(draw_color);
                    self.draw_dot(x as u32, y as u32);
                }
            }
        }
        Ok(())
    }

    pub fn draw(&mut self, cpu: &mut CPU) -> Result<(), String> {
        self.draw_background(cpu);
        self.draw_foreground(cpu)?;
        self.canvas.present();

        Ok(())
    }

    fn get_draw_color(&mut self, cpu: &mut CPU, palette_index: u8) -> Color {
        let color = cpu.palette()[palette_index as usize];
        return Color::RGB(
            (color >> 16) as u8,
            ((color >> 8) & 0xFF) as u8,
            (color & 0xFF) as u8,
        );
    }

    fn draw_background(&mut self, cpu: &mut CPU) {
        let bgc_index = cpu.bgc();
        let draw_color = self.get_draw_color(cpu, bgc_index);
        self.canvas.set_draw_color(draw_color);
        self.canvas.clear();
    }
}
