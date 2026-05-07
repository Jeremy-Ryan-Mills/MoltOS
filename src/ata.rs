/***
 * src/ata.rs
 *
 * ATA PIO driver for the primary IDE channel.
 * Supports LBA28 sector reads from master or slave.
 * Intended for bulk boot-time loads; not optimized for runtime throughput.
 */

use x86_64::instructions::port::Port;

const BASE:  u16 = 0x1F0; // primary channel data/register base
const CTRL:  u16 = 0x3F6; // alternate status / device control

// Register offsets from BASE
const DATA:   u16 = BASE + 0; // 16-bit data port
const _ERR:   u16 = BASE + 1;
const SECCNT: u16 = BASE + 2;
const LBA0:   u16 = BASE + 3;
const LBA1:   u16 = BASE + 4;
const LBA2:   u16 = BASE + 5;
const DRVSEL: u16 = BASE + 6;
const STATUS: u16 = BASE + 7;
const CMD:    u16 = BASE + 7;

const CMD_READ: u8 = 0x20;
const BSY: u8 = 0x80;
const DRQ: u8 = 0x08;
const ERR_BIT: u8 = 0x01;

#[derive(Clone, Copy)]
pub enum Drive { Master, Slave }

#[derive(Debug)]
pub enum AtaError {
    NoDrive,
    DiskError,
    Overflow,
}

// Read alternate status 4 times to burn ~400 ns for drive select to settle.
unsafe fn delay400ns() {
    let mut p: Port<u8> = Port::new(CTRL);
    for _ in 0..4 { unsafe { let _ = p.read(); } }
}

unsafe fn wait_ready() -> Result<(), AtaError> {
    let mut p: Port<u8> = Port::new(STATUS);
    loop {
        let s = unsafe { p.read() };
        if s == 0xFF { return Err(AtaError::NoDrive); }
        if s & ERR_BIT != 0 { return Err(AtaError::DiskError); }
        if s & BSY == 0 && s & DRQ != 0 { return Ok(()); }
    }
}

// Read `buf` from `drive` starting at `lba`.
// buf.len() must be a multiple of 512.
pub fn read(drive: Drive, lba: u32, buf: &mut [u8]) -> Result<(), AtaError> {
    assert!(buf.len() % 512 == 0);
    let sectors = (buf.len() / 512) as u32;
    if sectors == 0 { return Ok(()); }
    if sectors > 0x100 { return Err(AtaError::Overflow); } // LBA28 single command max

    let drv_bit: u8 = match drive { Drive::Master => 0, Drive::Slave => 1 };

    unsafe {
        // Select drive, upper 4 LBA bits
        Port::<u8>::new(DRVSEL).write(0xE0 | (drv_bit << 4) | ((lba >> 24) as u8 & 0x0F));
        delay400ns();

        Port::<u8>::new(SECCNT).write(if sectors == 256 { 0 } else { sectors as u8 });
        Port::<u8>::new(LBA0).write(lba as u8);
        Port::<u8>::new(LBA1).write((lba >> 8) as u8);
        Port::<u8>::new(LBA2).write((lba >> 16) as u8);
        Port::<u8>::new(CMD).write(CMD_READ);

        for chunk in buf.chunks_exact_mut(512) {
            wait_ready()?;
            let mut data: Port<u16> = Port::new(DATA);
            for word in chunk.chunks_exact_mut(2) {
                let v = data.read();
                word[0] = v as u8;
                word[1] = (v >> 8) as u8;
            }
        }
    }

    Ok(())
}

// Read the entire drive into a Vec, up to `max_bytes`.
// Stops early if a sector returns all 0x00 (sparse end of image).
pub fn read_all(drive: Drive, max_bytes: usize) -> Result<alloc::vec::Vec<u8>, AtaError> {
    extern crate alloc;
    use alloc::vec;

    let max_sectors = (max_bytes + 511) / 512;
    let mut buf = vec![0u8; max_sectors * 512];
    let mut end = 0;

    for i in (0..max_sectors).step_by(128) {
        let count = (max_sectors - i).min(128);
        let slice = &mut buf[i * 512..(i + count) * 512];
        match read(drive, i as u32, slice) {
            Ok(()) => { end = (i + count) * 512; }
            Err(AtaError::NoDrive) => break,
            Err(e) => return Err(e),
        }
    }

    buf.truncate(end);
    Ok(buf)
}
