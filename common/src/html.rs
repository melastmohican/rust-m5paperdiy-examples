//! A chapter of XHTML as a list of blocks: paragraphs of words with a style, and images. This is
//! the first half of what the Arduino reader's `RubbishHtmlParser` does; [`crate::layout`] is the
//! second half.
//!
//! Only what a page of prose needs is understood. Headings (`h1` to `h6`) start a centered bold
//! block; `p`, `li` and `div` start a justified block; `br` starts a block with the same alignment;
//! `b` and `strong` are bold, `i` and `em` are italic; `img` becomes an image block; `head`,
//! `script`, `style` and `table` are skipped with everything inside them. Words split at spaces,
//! tabs and line breaks only, so a no-break space stays inside its word.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_xml::{Token, Tokenizer, decode, local_name};

use crate::epub::resolve_path;

/// How the lines of a text block are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Spread over the full width, except the last line.
    Justified,
    Left,
    Center,
    Right,
}

/// A word of a text block: where it is in the block's text, and its style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Word {
    pub(crate) start: u32,
    pub(crate) len: u32,
    pub bold: bool,
    pub italic: bool,
}

/// A paragraph: its words, one after the other in one string, and how its lines are placed.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBlock {
    pub align: Align,
    pub(crate) text: String,
    pub(crate) words: Vec<Word>,
    /// Index of the word after the last word of each line, filled by the layout.
    pub(crate) line_breaks: Vec<u32>,
    /// Distance of each word from the left edge, filled by the layout.
    pub(crate) word_x: Vec<u16>,
}

impl TextBlock {
    fn new(align: Align) -> Self {
        Self {
            align,
            text: String::new(),
            words: Vec::new(),
            line_breaks: Vec::new(),
            word_x: Vec::new(),
        }
    }

    /// True when the block has no words.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Number of words.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// The text of word `index`.
    pub fn word(&self, index: usize) -> &str {
        let w = &self.words[index];
        &self.text[w.start as usize..(w.start + w.len) as usize]
    }

    /// The words of the block in order, with their style.
    pub fn words(&self) -> impl Iterator<Item = (&str, bool, bool)> {
        (0..self.words.len()).map(|i| (self.word(i), self.words[i].bold, self.words[i].italic))
    }

    /// Splits `text` into words and adds them in the given style.
    fn add_text(&mut self, text: &str, bold: bool, italic: bool) {
        for word in text.split(is_space).filter(|w| !w.is_empty()) {
            self.words.push(Word {
                start: self.text.len() as u32,
                len: word.len() as u32,
                bold,
                italic,
            });
            self.text.push_str(word);
        }
    }
}

/// An image, by its path inside the archive. The layout fills in its size and place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBlock {
    pub path: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) x: u32,
}

/// A piece of a chapter.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Text(TextBlock),
    Image(ImageBlock),
}

/// The white space of the Arduino reader: a space, a tab or a line break.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn is_heading(name: &str) -> bool {
    matches!(name.as_bytes(), [b'h' | b'H', b'1'..=b'6'])
}

/// What the parser is doing between two tokens.
struct Parser<'a> {
    base_dir: &'a str,
    /// The blocks that are finished.
    blocks: Vec<Block>,
    /// The text block words are added to.
    open: TextBlock,
    /// Open `b`, `strong` and heading elements, so nested ones do not switch bold off early.
    bold: u32,
    italic: u32,
    /// Depth of the element being skipped, 0 when nothing is.
    skip: u32,
}

impl Parser<'_> {
    /// Starts a new text block with `align`, or reuses the open one when it is still empty.
    fn start_block(&mut self, align: Align) {
        if self.open.is_empty() {
            self.open.align = align;
        } else {
            let finished = core::mem::replace(&mut self.open, TextBlock::new(align));
            self.blocks.push(Block::Text(finished));
        }
    }

    fn start(&mut self, name: &str, src: Option<String>, self_closing: bool) {
        let name = local_name(name);
        if matches!(name, "head" | "script" | "style" | "table") {
            if !self_closing {
                self.skip = 1;
            }
            return;
        }
        if name.eq_ignore_ascii_case("img") {
            if let Some(src) = src {
                // The text before the image is finished, and the text after it goes on with the
                // same alignment.
                let align = self.open.align;
                self.start_block(align);
                self.blocks.push(Block::Image(ImageBlock {
                    path: resolve_path(self.base_dir, &src),
                    width: 0,
                    height: 0,
                    x: 0,
                }));
            }
        } else if is_heading(name) {
            if !self_closing {
                self.bold += 1;
            }
            self.start_block(Align::Center);
        } else if name.eq_ignore_ascii_case("br") {
            let align = self.open.align;
            self.start_block(align);
        } else if ["p", "li", "div"]
            .iter()
            .any(|t| name.eq_ignore_ascii_case(t))
        {
            self.start_block(Align::Justified);
        } else if !self_closing {
            if name.eq_ignore_ascii_case("b") || name.eq_ignore_ascii_case("strong") {
                self.bold += 1;
            } else if name.eq_ignore_ascii_case("i") || name.eq_ignore_ascii_case("em") {
                self.italic += 1;
            }
        }
    }

    fn end(&mut self, name: &str) {
        let name = local_name(name);
        if is_heading(name) || name.eq_ignore_ascii_case("b") || name.eq_ignore_ascii_case("strong")
        {
            self.bold = self.bold.saturating_sub(1);
        } else if name.eq_ignore_ascii_case("i") || name.eq_ignore_ascii_case("em") {
            self.italic = self.italic.saturating_sub(1);
        }
    }

    fn text(&mut self, text: &str) {
        let (bold, italic) = (self.bold > 0, self.italic > 0);
        self.open.add_text(text, bold, italic);
    }
}

/// Reads a chapter. `base_dir` is the directory of the chapter inside the archive, with a trailing
/// `/` (see [`crate::epub::dir_of`]), so image paths resolve against it.
pub fn parse(xhtml: &[u8], base_dir: &str) -> Vec<Block> {
    let mut parser = Parser {
        base_dir,
        blocks: Vec::new(),
        open: TextBlock::new(Align::Justified),
        bold: 0,
        italic: 0,
        skip: 0,
    };
    let mut emit = |token: Token<'_>| match token {
        Token::Start {
            name,
            attrs,
            self_closing,
        } => {
            if parser.skip > 0 {
                if !self_closing {
                    parser.skip += 1;
                }
                return;
            }
            let src = attrs.get("src").map(|s| decode(&s).into_owned());
            parser.start(name, src, self_closing);
        }
        Token::End { .. } if parser.skip > 0 => parser.skip -= 1,
        Token::End { name } => parser.end(name),
        Token::Text(text) if parser.skip == 0 => parser.text(&decode(&text)),
        Token::CData(text) if parser.skip == 0 => parser.text(text),
        _ => {}
    };
    let mut tokenizer = Tokenizer::html();
    tokenizer.feed(xhtml, &mut emit);
    tokenizer.finish(&mut emit);
    if !parser.open.is_empty() {
        let last = core::mem::replace(&mut parser.open, TextBlock::new(Align::Justified));
        parser.blocks.push(Block::Text(last));
    }
    parser.blocks
}
