use serde::Serialize;

#[derive(Clone, Copy)]
pub enum ContentType {
    Email,
    Article,
    Pdf,
}

impl TryFrom<&str> for ContentType {
    type Error = &'static str;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "email" => Ok(Self::Email),
            "article" => Ok(Self::Article),
            "pdf" => Ok(Self::Pdf),
            _ => Err("unknown content type"),
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chunk {
    pub text: String,
    pub index: usize,
    pub token_count: usize,
    pub start: usize,
    pub end: usize,
}

pub fn chunk_for_embedding(text: &str, content_type: ContentType) -> Vec<Chunk> {
    let text = normalize(text);
    let (size, overlap) = match content_type {
        ContentType::Email => (800, 120),
        ContentType::Article => (1200, 200),
        ContentType::Pdf => (1600, 240),
    };
    let mut chunks = Vec::new();
    let (mut start_byte, mut start_units) = (0, 0);
    while start_byte < text.len() {
        let (mut end_byte, mut end_units) = (start_byte, start_units);
        for (offset, ch) in text[start_byte..].char_indices() {
            if end_units - start_units + ch.len_utf16() > size {
                break;
            }
            end_byte = start_byte + offset + ch.len_utf8();
            end_units += ch.len_utf16();
        }
        chunks.push(Chunk {
            text: text[start_byte..end_byte].to_owned(),
            index: chunks.len(),
            token_count: (end_units - start_units).div_ceil(4),
            start: start_units,
            end: end_units,
        });
        if end_byte == text.len() {
            break;
        }

        let (mut next_byte, mut next_units) = (end_byte, end_units);
        for (offset, ch) in text[start_byte..end_byte].char_indices().rev() {
            if end_units - next_units + ch.len_utf16() > overlap {
                break;
            }
            next_byte = start_byte + offset;
            next_units -= ch.len_utf16();
        }
        start_byte = next_byte;
        start_units = next_units;
    }
    chunks
}

fn normalize(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut newlines = 0;
    while let Some(mut ch) = chars.next() {
        if ch == '\r' && chars.peek() == Some(&'\n') {
            chars.next();
            ch = '\n';
        }
        if ch == '\t' {
            ch = ' ';
        }
        if ch == ' ' && output.ends_with(' ') {
            continue;
        }
        if ch == '\n' {
            if newlines == 2 {
                continue;
            }
            newlines += 1;
        } else {
            newlines = 0;
        }
        output.push(ch);
    }
    output.trim_matches(js_whitespace).to_owned()
}

fn js_whitespace(ch: char) -> bool {
    matches!(ch,
        '\u{0009}'..='\u{000D}' | ' ' | '\u{00A0}' | '\u{1680}' |
        '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' |
        '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}
