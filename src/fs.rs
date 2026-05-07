/***
 * src/fs.rs
 *
 * CXFS: minimal flat archive filesystem.
 *
 * Format:
 *   [0..4]   magic  "CXFS"
 *   [4..8]   u32 LE file count N
 *   [8..]    N * 40-byte entries
 *     [+0..+32]  filename, null-terminated, zero-padded
 *     [+32..+36] u32 LE data offset from archive start
 *     [+36..+40] u32 LE data size in bytes
 *   [8 + N*40 ..] packed file data
 *
 * The archive is loaded from the ATA slave at boot and kept in a heap Vec.
 * After init(), open() returns a &'static [u8] into that buffer.
 */

extern crate alloc;
use alloc::vec::Vec;
use conquer_once::spin::OnceCell;
use crate::ata::{self, Drive};

const MAGIC: &[u8; 4] = b"CXFS";
const ENTRY_SIZE: usize = 40;
const NAME_LEN:   usize = 32;

static ARCHIVE: OnceCell<Vec<u8>> = OnceCell::uninit();

#[derive(Debug)]
pub enum FsError {
    NoDrive,
    DiskError,
    BadMagic,
}

pub fn init() -> Result<usize, FsError> {
    // 16 MiB ceiling; a DOOM1.WAD + overhead fits easily.
    let data = ata::read_all(Drive::Slave, 16 * 1024 * 1024)
        .map_err(|e| match e {
            ata::AtaError::NoDrive => FsError::NoDrive,
            _ => FsError::DiskError,
        })?;

    if data.len() < 8 || &data[0..4] != MAGIC {
        return Err(FsError::BadMagic);
    }

    let count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    ARCHIVE.try_init_once(|| data).ok();
    Ok(count)
}

pub fn open(name: &str) -> Option<&'static [u8]> {
    let data: &'static [u8] = ARCHIVE.try_get().ok()?;
    let count = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;

    for i in 0..count {
        let entry = &data[8 + i * ENTRY_SIZE..][..ENTRY_SIZE];
        let raw_name = &entry[..NAME_LEN];
        let nul = raw_name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        let entry_name = core::str::from_utf8(&raw_name[..nul]).ok()?;
        if entry_name == name {
            let offset = u32::from_le_bytes(entry[32..36].try_into().ok()?) as usize;
            let size   = u32::from_le_bytes(entry[36..40].try_into().ok()?) as usize;
            return Some(&data[offset..offset + size]);
        }
    }

    None
}
