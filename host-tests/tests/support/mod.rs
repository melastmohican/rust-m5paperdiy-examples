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

/// A zip archive of `(name, content)` pairs. Entries named in `deflated` are compressed, the rest are
/// stored, so both paths of the reader are covered.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

pub fn zip(files: &[(&str, &str)], deflated: &[&str]) -> Vec<u8> {
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, content)| (*name, content.as_bytes()))
        .collect();
    zip_bytes(&files, deflated)
}

/// As [`zip`], for files that are not text, like pictures.
pub fn zip_bytes(files: &[(&str, &[u8])], deflated: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, raw) in files {
        let raw = *raw;
        let (method, data) = if deflated.contains(name) {
            (8u16, miniz_oxide::deflate::compress_to_vec(raw, 6))
        } else {
            (0u16, raw.to_vec())
        };
        let crc = crc32(raw);
        let offset = out.len() as u32;
        let mut common = Vec::new();
        common.extend_from_slice(&20u16.to_le_bytes()); // version needed
        common.extend_from_slice(&0u16.to_le_bytes()); // flags
        common.extend_from_slice(&method.to_le_bytes());
        common.extend_from_slice(&0u32.to_le_bytes()); // time and date
        common.extend_from_slice(&crc.to_le_bytes());
        common.extend_from_slice(&(data.len() as u32).to_le_bytes());
        common.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        common.extend_from_slice(&(name.len() as u16).to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes()); // extra length
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&common);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&common);
        central.extend_from_slice(&0u16.to_le_bytes()); // comment length
        central.extend_from_slice(&0u16.to_le_bytes()); // disk number
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let central_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// A small valid EPUB 2: `chapters` are `(title, body)` in reading order, each in its own XHTML file,
/// all listed in the table of contents (an NCX).
pub fn simple_epub(title: &str, chapters: &[(&str, &str)]) -> Vec<u8> {
    let container = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    let mut manifest =
        String::from(r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#);
    let mut spine = String::new();
    let mut nav = String::new();
    let mut files: Vec<(String, String)> = Vec::new();
    for (i, (name, body)) in chapters.iter().enumerate() {
        manifest.push_str(&format!(
            r#"<item id="c{i}" href="ch{i}.xhtml" media-type="application/xhtml+xml"/>"#
        ));
        spine.push_str(&format!(r#"<itemref idref="c{i}"/>"#));
        nav.push_str(&format!(
            r#"<navPoint id="n{i}" playOrder="{}"><navLabel><text>{name}</text></navLabel><content src="ch{i}.xhtml"/></navPoint>"#,
            i + 1
        ));
        files.push((
            format!("OEBPS/ch{i}.xhtml"),
            format!("<html><body>{body}</body></html>"),
        ));
    }
    let opf = format!(
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title></metadata><manifest>{manifest}</manifest><spine toc="ncx">{spine}</spine></package>"#
    );
    let ncx = format!(
        r#"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap>{nav}</navMap></ncx>"#
    );
    let mut entries: Vec<(&str, &str)> = vec![
        ("META-INF/container.xml", container),
        ("OEBPS/content.opf", &opf),
        ("OEBPS/toc.ncx", &ncx),
    ];
    for (name, content) in &files {
        entries.push((name, content));
    }
    zip(&entries, &["OEBPS/content.opf"])
}
