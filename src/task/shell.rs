//! Simple terminal / command loop.
//!
//! Reads keyboard input line-by-line, parses the first token as a command,
//! and runs built-in commands (help, clear, echo).

use alloc::string::String;
use alloc::vec::Vec;
use core::str;
use futures_util::StreamExt;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

use crate::print;
use crate::println;
use crate::vga_buffer;
use super::keyboard::ScancodeStream;

const PROMPT: &str = "chronos> ";

/// Runs the shell loop: prompt, read line, execute command, repeat.
pub async fn run_shell() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        HandleControl::Ignore,
    );
    let mut line = String::new();

    println!("Chronos shell. Type 'help' for commands.");
    print!("{}", PROMPT);

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    // ENTER: handled here (decoded)
                    DecodedKey::Unicode('\r') | DecodedKey::Unicode('\n') => {
                        println!();
                        run_command(line.trim());
                        line.clear();
                        print!("{}", PROMPT);
                    }

                    // Regular printable characters
                    DecodedKey::Unicode(c) => {
                        // Backspace/DEL often come through as control chars
                        if c == '\x08' || c == '\x7f' {
                            if line.pop().is_some() {
                                print!("\x08");
                            }
                        } else if c.is_ascii() && !c.is_control() {
                            line.push(c);
                            print!("{}", c);
                        }
                    }

                    // Non-character keys (arrows, backspace, etc.)
                    DecodedKey::RawKey(key) => {
                        use pc_keyboard::KeyCode;
                        if let KeyCode::Backspace = key {
                            if line.pop().is_some() {
                                print!("\x08");
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Parse the line and dispatch to a built-in command.
fn run_command(line: &str) {
    if line.is_empty() {
        return;
    }

    let mut parts = line.split_ascii_whitespace();
    let cmd = parts.next().unwrap_or("");
    let rest: String = parts.collect::<Vec<_>>().join(" ");

    match cmd {
        "help" => cmd_help(),
        "clear" | "cls" => cmd_clear(),
        "echo" => cmd_echo(rest.trim()),
        "uptime" => cmd_uptime(),
        "mem" | "memory" => cmd_mem(),
        _ => println!("unknown command: '{}'. Type 'help' for commands.", cmd),
    }
}

fn cmd_help() {
    println!("  help       - show this message");
    println!("  clear/cls  - clear the screen");
    println!("  echo ...   - print the rest of the line");
    println!("  uptime     - show uptime in timer ticks");
    println!("  mem/memory - dump memory map");
}

fn cmd_uptime() {
    println!("{} ticks", crate::uptime_ticks());
}

fn cmd_mem() {
    println!("Memory map:");
    crate::memory::dump_memory_map();
}

fn cmd_clear() {
    vga_buffer::clear_screen();
}

fn cmd_echo(args: &str) {
    println!("{}", args);
}
