//! Helpers shared by the test files.

#![allow(dead_code)]

/// A FAT32 disk with an MBR and one empty partition, 40 MiB, 512 byte clusters. `embedded-sdmmc`
/// tells FAT types apart by cluster count, so it needs at least 65,525 clusters to read FAT32.
pub fn fat32_image() -> Vec<u8> {
    const SECTORS: usize = 81_920;
    const START: usize = 2048;
    const PART: usize = SECTORS - START;
    const RESERVED: usize = 32;
    const FAT_SECTORS: usize = 624;

    fn put16(buf: &mut [u8], at: usize, value: u16) {
        buf[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(buf: &mut [u8], at: usize, value: u32) {
        buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    let mut img = vec![0u8; SECTORS * 512];
    // MBR: one FAT32 LBA partition.
    img[446 + 4] = 0x0C;
    put32(&mut img, 446 + 8, START as u32);
    put32(&mut img, 446 + 12, PART as u32);
    img[510] = 0x55;
    img[511] = 0xAA;

    // The boot sector, written at the start of the partition and again at sector 6.
    let mut boot = [0u8; 512];
    boot[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
    boot[3..11].copy_from_slice(b"MSWIN4.1");
    put16(&mut boot, 11, 512); // bytes per sector
    boot[13] = 1; // sectors per cluster
    put16(&mut boot, 14, RESERVED as u16);
    boot[16] = 2; // FATs
    boot[21] = 0xF8; // media
    put16(&mut boot, 24, 63);
    put16(&mut boot, 26, 255);
    put32(&mut boot, 28, START as u32); // hidden sectors
    put32(&mut boot, 32, PART as u32);
    put32(&mut boot, 36, FAT_SECTORS as u32);
    put32(&mut boot, 44, 2); // root cluster
    put16(&mut boot, 48, 1); // FSInfo sector
    put16(&mut boot, 50, 6); // backup boot sector
    boot[64] = 0x80;
    boot[66] = 0x29;
    put32(&mut boot, 67, 0x1234_ABCD);
    boot[71..82].copy_from_slice(b"TEST       ");
    boot[82..90].copy_from_slice(b"FAT32   ");
    boot[510] = 0x55;
    boot[511] = 0xAA;
    let base = START * 512;
    img[base..base + 512].copy_from_slice(&boot);
    img[base + 6 * 512..base + 7 * 512].copy_from_slice(&boot);

    let mut info = [0u8; 512];
    put32(&mut info, 0, 0x4161_5252);
    put32(&mut info, 484, 0x6141_7272);
    put32(&mut info, 488, 0xFFFF_FFFF);
    put32(&mut info, 492, 0xFFFF_FFFF);
    info[510] = 0x55;
    info[511] = 0xAA;
    img[base + 512..base + 1024].copy_from_slice(&info);

    // Both FATs: the media entry, the end marker and the root directory's single cluster.
    for fat in 0..2 {
        let at = base + (RESERVED + fat * FAT_SECTORS) * 512;
        put32(&mut img, at, 0x0FFF_FFF8);
        put32(&mut img, at + 4, 0x0FFF_FFFF);
        put32(&mut img, at + 8, 0x0FFF_FFFF);
    }
    img
}

/// Where `UI_DUMP` points, if it is set: the tests write what they draw there.
pub fn dump_dir() -> Option<String> {
    std::env::var("UI_DUMP").ok().filter(|dir| !dir.is_empty())
}
