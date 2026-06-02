// CXFS flat archive: magic "CXFS", u32 LE count, N*40-byte entries
// (32-byte name, u32 offset, u32 size), then packed file data.
// Loaded from ATA slave at boot; open() returns a &'static [u8] into the buffer.

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
    // 6 MiB ceiling: DOOM1.WAD shareware is ~4.2 MiB; leaves heap headroom.
    let data = ata::read_all(Drive::Slave, 6 * 1024 * 1024)
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
        if entry_name.eq_ignore_ascii_case(name) {
            let offset = u32::from_le_bytes(entry[32..36].try_into().ok()?) as usize;
            let size   = u32::from_le_bytes(entry[36..40].try_into().ok()?) as usize;
            return Some(&data[offset..offset + size]);
        }
    }

    None
}
