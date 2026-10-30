//! The container, the package (OPF) and the table of contents of an EPUB book, read from a zip
//! archive in a [`Storage`]. Nothing here touches the hardware: on the board the storage is the
//! microSD card, in the host tests it is a file or a block of memory.
//!
//! An EPUB is a zip archive. `META-INF/container.xml` names the package document, the package
//! lists the files (manifest) and the reading order (spine), and the table of contents is an NCX
//! file (EPUB 2) or a `nav` document (EPUB 3). The reader follows the Arduino reader: the NCX is
//! used when the spine names one, the `nav` document otherwise.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_platform::storage::Storage;
use paperpaw_xml::{Token, Tokenizer, decode, local_name};
use paperpaw_zip::{ZipArchive, ZipError};

/// Largest XML or XHTML document read into memory, in bytes.
pub const MAX_DOCUMENT: usize = 2 << 20;

/// Why a book could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EpubError {
    /// The zip archive could not be read.
    Zip(ZipError),
    /// A file the format requires is not in the archive.
    Missing(&'static str),
    /// A file is there but does not say what it must.
    Invalid(&'static str),
    /// A spine index past the last item.
    OutOfRange,
}

impl fmt::Display for EpubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EpubError::Zip(e) => write!(f, "zip: {e}"),
            EpubError::Missing(what) => write!(f, "missing {what}"),
            EpubError::Invalid(what) => write!(f, "invalid {what}"),
            EpubError::OutOfRange => f.write_str("no such spine item"),
        }
    }
}

impl core::error::Error for EpubError {}

impl From<ZipError> for EpubError {
    fn from(e: ZipError) -> Self {
        EpubError::Zip(e)
    }
}

/// One line of the table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// The text of the link, with entities decoded and white space collapsed.
    pub title: String,
    /// Nesting depth, 0 for the top level.
    pub level: u8,
    /// Index into [`Book::spine`] of the document it points at, when that document is in the spine.
    pub spine_index: Option<usize>,
    /// The part after `#` in the link, when there is one.
    pub fragment: Option<String>,
}

/// An opened book: its title, reading order and table of contents. The archive is read again for
/// every document, so only this index stays in memory.
pub struct Book {
    title: String,
    spine: Vec<String>,
    toc: Vec<TocEntry>,
    zip: ZipArchive,
}

impl Book {
    /// Opens the book at `path` in `storage`.
    pub fn open(storage: &dyn Storage, path: &str) -> Result<Self, EpubError> {
        let zip = ZipArchive::open(storage, path)?;
        let container = read_file(&zip, storage, "META-INF/container.xml")?
            .ok_or(EpubError::Missing("META-INF/container.xml"))?;
        let package_path = package_path(&container)
            .ok_or(EpubError::Invalid("container.xml without a rootfile"))?;
        let package = read_file(&zip, storage, &package_path)?
            .ok_or(EpubError::Missing("package document"))?;
        let opf = parse_package(&package);
        let base = dir_of(&package_path);

        let spine: Vec<String> = opf
            .spine
            .iter()
            .filter_map(|id| opf.manifest.get(id))
            .map(|item| resolve_path(base, &item.href))
            .collect();

        let toc = read_toc(&zip, storage, base, &opf, &spine)?;
        Ok(Self {
            title: opf.title,
            spine,
            toc,
            zip,
        })
    }

    /// The book's title, empty when the package has none.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The paths inside the archive of the documents in reading order.
    pub fn spine(&self) -> &[String] {
        &self.spine
    }

    /// The table of contents, in reading order, empty when the book has none.
    pub fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    /// Reads spine document `index`.
    pub fn read_spine(&self, storage: &dyn Storage, index: usize) -> Result<Vec<u8>, EpubError> {
        let path = self.spine.get(index).ok_or(EpubError::OutOfRange)?;
        self.read_path(storage, path)
    }

    /// Reads any file of the archive by its path, for example an image a document refers to.
    pub fn read_path(&self, storage: &dyn Storage, path: &str) -> Result<Vec<u8>, EpubError> {
        read_file(&self.zip, storage, path)?.ok_or(EpubError::Missing("file in the archive"))
    }
}

/// The table of contents of the book. A book without one, or whose file is missing, gets an empty one.
fn read_toc(
    zip: &ZipArchive,
    storage: &dyn Storage,
    base: &str,
    opf: &Package,
    spine: &[String],
) -> Result<Vec<TocEntry>, EpubError> {
    let ncx = opf.ncx_id.as_ref().and_then(|id| opf.manifest.get(id));
    let nav = opf
        .manifest
        .values()
        .find(|item| item.properties.split_whitespace().any(|p| p == "nav"));
    let (item, is_ncx) = match (ncx, nav) {
        (Some(item), _) => (item, true),
        (None, Some(item)) => (item, false),
        (None, None) => return Ok(Vec::new()),
    };
    let path = resolve_path(base, &item.href);
    let Some(data) = read_file(zip, storage, &path)? else {
        return Ok(Vec::new());
    };
    let raw = if is_ncx {
        parse_ncx(&data)
    } else {
        parse_nav(&data)
    };
    let toc_dir = dir_of(&path);
    Ok(raw
        .into_iter()
        .filter(|entry| !entry.title.is_empty())
        .map(|entry| {
            let (file, fragment) = match entry.href.split_once('#') {
                Some((file, fragment)) => (file, Some(percent_decode(fragment))),
                None => (entry.href.as_str(), None),
            };
            let spine_index = if file.is_empty() {
                None
            } else {
                let full = resolve_path(toc_dir, file);
                spine.iter().position(|item| *item == full)
            };
            TocEntry {
                title: entry.title,
                level: entry.level,
                spine_index,
                fragment: fragment.filter(|f| !f.is_empty()),
            }
        })
        .collect())
}

/// Reads one file of the archive, `None` when there is no such file.
fn read_file(
    zip: &ZipArchive,
    storage: &dyn Storage,
    name: &str,
) -> Result<Option<Vec<u8>>, ZipError> {
    match zip.entry(storage, name)? {
        Some(entry) => zip.read(storage, &entry, MAX_DOCUMENT).map(Some),
        None => Ok(None),
    }
}

/// `full-path` of the first `rootfile` in `META-INF/container.xml`.
fn package_path(container: &[u8]) -> Option<String> {
    let mut found: Option<String> = None;
    Tokenizer::parse(container, &mut |token| {
        if let Token::Start { name, attrs, .. } = token
            && found.is_none()
            && local_name(name) == "rootfile"
        {
            found = attrs
                .get("full-path")
                .map(|path| decode(&path).into_owned());
        }
    });
    found
}

/// A file listed in the manifest.
struct Item {
    href: String,
    properties: String,
}

/// What the package document says.
#[derive(Default)]
struct Package {
    title: String,
    manifest: BTreeMap<String, Item>,
    /// `idref` of each `itemref`, in order.
    spine: Vec<String>,
    /// The `toc` attribute of the spine: the manifest id of the NCX.
    ncx_id: Option<String>,
}

fn parse_package(data: &[u8]) -> Package {
    let mut opf = Package::default();
    let mut in_title = false;
    let mut title_done = false;
    Tokenizer::parse(data, &mut |token| match token {
        Token::Start { name, attrs, .. } => match local_name(name) {
            "title" if !title_done => in_title = true,
            "item" => {
                let get = |key: &str| {
                    attrs
                        .get(key)
                        .map(|value| decode(&value).into_owned())
                        .unwrap_or_default()
                };
                opf.manifest.insert(
                    get("id"),
                    Item {
                        href: get("href"),
                        properties: get("properties"),
                    },
                );
            }
            "spine" => opf.ncx_id = attrs.get("toc").map(|id| decode(&id).into_owned()),
            "itemref" => {
                if let Some(id) = attrs.get("idref") {
                    opf.spine.push(decode(&id).into_owned());
                }
            }
            _ => {}
        },
        Token::Text(text) if in_title => opf.title.push_str(&decode(&text)),
        Token::End { name } if local_name(name) == "title" && in_title => {
            in_title = false;
            title_done = true;
        }
        _ => {}
    });
    opf.title = collapse_spaces(&opf.title);
    opf
}

/// A link of a table of contents as written, before the title is cleaned and the target resolved.
struct RawEntry {
    title: String,
    level: u8,
    href: String,
}

/// EPUB 2: `navPoint` elements, nested, each with a `navLabel/text` and a `content src`.
fn parse_ncx(data: &[u8]) -> Vec<RawEntry> {
    let mut out: Vec<RawEntry> = Vec::new();
    // Indices into `out` of the navPoints that are open.
    let mut open: Vec<usize> = Vec::new();
    let (mut in_label, mut in_text) = (false, false);
    Tokenizer::parse(data, &mut |token| match token {
        Token::Start { name, attrs, .. } => match local_name(name) {
            "navPoint" => {
                out.push(RawEntry {
                    title: String::new(),
                    level: u8::try_from(open.len()).unwrap_or(u8::MAX),
                    href: String::new(),
                });
                open.push(out.len() - 1);
            }
            "navLabel" => in_label = true,
            "text" if in_label => in_text = true,
            "content" => {
                if let (Some(&i), Some(src)) = (open.last(), attrs.get("src")) {
                    out[i].href = decode(&src).into_owned();
                }
            }
            _ => {}
        },
        Token::Text(text) if in_text => {
            if let Some(&i) = open.last() {
                out[i].title.push_str(&decode(&text));
            }
        }
        Token::End { name } => match local_name(name) {
            "navPoint" => {
                open.pop();
            }
            "navLabel" => in_label = false,
            "text" => in_text = false,
            _ => {}
        },
        _ => {}
    });
    for entry in &mut out {
        entry.title = collapse_spaces(&entry.title);
    }
    out
}

/// EPUB 3: the links of the `nav` element whose `epub:type` is `toc`, or of the first `nav`
/// when none says so. Nested lists give the nesting depth.
fn parse_nav(data: &[u8]) -> Vec<RawEntry> {
    // (index of the nav element, the link)
    let mut links: Vec<(usize, RawEntry)> = Vec::new();
    let mut navs = 0usize;
    let mut in_nav: Option<usize> = None;
    let mut toc_nav: Option<usize> = None;
    let mut depth = 0u8;
    let mut open_link = false;
    Tokenizer::parse(data, &mut |token| match token {
        Token::Start { name, attrs, .. } => match local_name(name) {
            "nav" => {
                in_nav = Some(navs);
                depth = 0;
                let kind = attrs.get("epub:type").or_else(|| attrs.get("type"));
                if toc_nav.is_none()
                    && kind.is_some_and(|k| k.split_whitespace().any(|w| w == "toc"))
                {
                    toc_nav = Some(navs);
                }
                navs += 1;
            }
            "ol" | "ul" if in_nav.is_some() => depth = depth.saturating_add(1),
            "a" => {
                if let (Some(nav), Some(href)) = (in_nav, attrs.get("href")) {
                    links.push((
                        nav,
                        RawEntry {
                            title: String::new(),
                            level: depth.saturating_sub(1),
                            href: decode(&href).into_owned(),
                        },
                    ));
                    open_link = true;
                }
            }
            _ => {}
        },
        Token::Text(text) if open_link => {
            if let Some((_, link)) = links.last_mut() {
                link.title.push_str(&decode(&text));
            }
        }
        Token::End { name } => match local_name(name) {
            "a" => open_link = false,
            "ol" | "ul" if in_nav.is_some() => depth = depth.saturating_sub(1),
            "nav" => in_nav = None,
            _ => {}
        },
        _ => {}
    });
    let chosen = toc_nav.unwrap_or(0);
    links
        .into_iter()
        .filter(|(nav, _)| *nav == chosen)
        .map(|(_, mut entry)| {
            entry.title = collapse_spaces(&entry.title);
            entry
        })
        .collect()
}

/// Runs of white space become one space, and the ends are trimmed.
fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// The directory part of an archive path, with its trailing `/`, empty at the top.
pub fn dir_of(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..=i])
}

/// Path of `href` (as written in a document) inside the archive, when the document is in `base`
/// (a directory with a trailing `/`). Follows `.` and `..`, and decodes `%20` and the like.
pub fn resolve_path(base: &str, href: &str) -> String {
    let href = percent_decode(href);
    let joined = if let Some(rooted) = href.strip_prefix('/') {
        String::from(rooted)
    } else {
        let mut s = String::from(base);
        s.push_str(&href);
        s
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Decodes `%XX` escapes. Bytes that do not form valid UTF-8 are replaced.
pub fn percent_decode(text: &str) -> String {
    if !text.contains('%') {
        return String::from(text);
    }
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
