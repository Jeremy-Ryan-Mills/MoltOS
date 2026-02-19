//! VGA text-mode buffer driver.
//!
//! This module provides a safe(ish) abstraction for writing text to the VGA
//! text buffer at memory address `0xb8000` in 80x25 text mode.
//!
//! It supports:
//! - Colored text output
//! - Line wrapping and scrolling
//! - `print!` / `println!` macros similar to the Rust standard library
//!
//! All writes to VGA memory are performed using volatile accesses to ensure
//! the compiler does not optimize them away.

use core::fmt;
use lazy_static::lazy_static;
use spin::Mutex;
use volatile::Volatile;

/// Number of text rows in VGA text mode.
const BUFFER_HEIGHT: usize = 25;

/// Number of text columns in VGA text mode.
const BUFFER_WIDTH: usize = 80;

/// Prints formatted text to the VGA buffer without a trailing newline.
///
/// This macro behaves similarly to `std::print!`, but writes directly to the
/// VGA text buffer instead of stdout.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

/// Prints formatted text to the VGA buffer with a trailing newline.
///
/// This macro behaves similarly to `std::println!`.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Internal print function used by the `print!` and `println!` macros.
///
/// This function acquires the global VGA writer lock and forwards the
/// formatted output to it.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}

/// Clears the VGA text buffer and moves the cursor to the top-left.
pub fn clear_screen() {
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        WRITER.lock().clear_screen();
    });
}

/// VGA color values.
///
/// These correspond to the standard VGA text-mode color palette.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// A packed VGA color code combining foreground and background colors.
///
/// The lower 4 bits represent the foreground color, and the upper 4 bits
/// represent the background color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct ColorCode(u8);

impl ColorCode {
    /// Creates a new `ColorCode` from a foreground and background color.
    fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

/// A single character in the VGA text buffer.
///
/// Each screen character consists of an ASCII byte and a color code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

/// Representation of the VGA text buffer.
///
/// The buffer is a 2D array of `ScreenChar`s laid out exactly as expected
/// by the VGA hardware.
#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

/// A writer type for the VGA text buffer.
///
/// Maintains the current cursor position and color state, and provides
/// methods for writing bytes and strings to the screen.
pub struct Writer {
    /// Current column position on the last row.
    column_position: usize,

    /// Current foreground/background color.
    color_code: ColorCode,

    /// Reference to the VGA text buffer.
    buffer: &'static mut Buffer,
}

impl Writer {
    /// Writes a single byte to the VGA buffer.
    ///
    /// Printable ASCII bytes are written directly. Newlines cause the screen
    /// to scroll.
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            b'\x08' => self.backspace(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }

                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                self.buffer.chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code: self.color_code,
                });
                self.column_position += 1;
            }
        }
    }

    /// Moves the cursor back one column and erases the character there.
    fn backspace(&mut self) {
        if self.column_position > 0 {
            self.column_position -= 1;
            let row = BUFFER_HEIGHT - 1;
            let col = self.column_position;
            self.buffer.chars[row][col].write(ScreenChar {
                ascii_character: b' ',
                color_code: self.color_code,
            });
        }
    }

    /// Clears the entire screen and moves the cursor to the top-left.
    pub fn clear_screen(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row);
        }
        self.column_position = 0;
    }

    /// Advances the buffer to a new line, scrolling the screen if necessary.
    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer.chars[row][col].read();
                self.buffer.chars[row - 1][col].write(character);
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    /// Clears a row by filling it with blank characters.
    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
    }
}

impl Writer {
    /// Writes a string to the VGA buffer.
    ///
    /// Non-printable bytes are replaced with `0xfe`.
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                0x20..=0x7e | b'\n' | b'\x08' => self.write_byte(byte),
                _ => self.write_byte(0xfe),
            }
        }
    }
}

/// Allows the VGA writer to be used with Rust’s formatting infrastructure.
impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

// Global VGA text buffer writer.
//
// This is protected by a spinlock to allow safe concurrent access from
// different execution contexts (e.g. interrupts).
//
// # Safety
// The memory address `0xb8000` must be mapped and correspond to a VGA
// text buffer in the current execution environment.
lazy_static! {
    pub static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        column_position: 0,
        color_code: ColorCode::new(Color::Yellow, Color::Black),
        buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },
    });
}

#[test_case]
fn test_println_simple() {
    println!("test_println_simple output");
}

#[test_case]
fn test_println_many() {
    for _ in 0..200 {
        println!("test_println_many output");
    }
}

#[test_case]
fn test_println_output() {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    let s = "Some test string that fits on a single line";
    interrupts::without_interrupts(|| {
        let mut writer = WRITER.lock();
        writeln!(writer, "\n{}", s).expect("writeln failed");
        for (i, c) in s.chars().enumerate() {
            let screen_char = writer.buffer.chars[BUFFER_HEIGHT - 2][i].read();
            assert_eq!(char::from(screen_char.ascii_character), c);
        }
    });
}
