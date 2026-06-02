use alloc::string::String;
use alloc::vec::Vec;
use futures_util::StreamExt;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

use crate::print;
use crate::println;
use crate::vga_buffer;
use super::keyboard::ScancodeStream;
#[cfg(feature = "doom")]
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "doom")]
static DOOM_LAUNCHED: AtomicBool = AtomicBool::new(false);

const PROMPT: &str = "moltos> ";

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
                    DecodedKey::Unicode('\r') | DecodedKey::Unicode('\n') => {
                        println!();
                        run_command(line.trim());
                        line.clear();
                        print!("{}", PROMPT);
                    }
                    DecodedKey::Unicode(c) => {
                        if c == '\x08' || c == '\x7f' {
                            if line.pop().is_some() {
                                print!("\x08");
                            }
                        } else if c.is_ascii() && !c.is_control() {
                            line.push(c);
                            print!("{}", c);
                        }
                    }
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
        "syscall" => cmd_syscall(rest.trim()),
        #[cfg(feature = "doom")]
        "doom" => cmd_doom(),
        _ => println!("unknown command: '{}'. Type 'help' for commands.", cmd),
    }
}

fn cmd_help() {
    println!("  help       - show this message");
    println!("  clear/cls  - clear the screen");
    println!("  echo ...   - print the rest of the line");
    println!("  uptime     - show uptime in timer ticks");
    println!("  mem/memory - dump memory map");
    println!("  syscall    - test syscalls (uptime, putchar)");
    #[cfg(feature = "doom")]
    println!("  doom       - launch Doom (requires doom1.wad)");
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

fn cmd_syscall(args: &str) {
    use crate::syscall::{Syscall, syscall0, syscall1};
    let sub = args.split_ascii_whitespace().next().unwrap_or("");
    match sub {
        "uptime" | "u" => {
            let ticks = syscall0(Syscall::Uptime.as_u64());
            println!("syscall(SYS_UPTIME) = {} ticks", ticks);
        }
        "putchar" | "p" => {
            syscall1(Syscall::PutChar.as_u64(), b'A' as u64);
            println!(" (printed via syscall)");
        }
        "" => {
            println!("Usage: syscall <uptime|putchar>");
            println!("  uptime (u)  - call SYS_UPTIME via int 0x80");
            println!("  putchar (p) - call SYS_PUTCHAR('A') via int 0x80");
        }
        _ => println!("unknown syscall test: '{}'. Use uptime or putchar.", sub),
    }
}

#[cfg(feature = "doom")]
fn cmd_doom() {
    if DOOM_LAUNCHED.swap(true, Ordering::SeqCst) {
        println!("doom: already running");
        return;
    }
    println!("launching doom...");
    fn doom_entry() { crate::ffi::doom_thread_entry(); }
    crate::thread::SCHEDULER.lock().spawn(
        crate::thread::Thread::with_stack_size(doom_entry, 24 * 1024 * 1024)
    );
}
