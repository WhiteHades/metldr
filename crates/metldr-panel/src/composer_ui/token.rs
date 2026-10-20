#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Source,
    Command,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Token<'a> {
    pub(super) kind: Kind,
    pub(super) start: usize,
    pub(super) query: &'a str,
}

pub(super) fn parse(draft: &str) -> Option<Token<'_>> {
    let query_start = draft
        .char_indices()
        .rev()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        .last()
        .map_or(draft.len(), |(offset, _)| offset);
    let (start, symbol) = draft[..query_start].char_indices().next_back()?;
    let kind = match symbol {
        '@' => Kind::Source,
        '/' => Kind::Command,
        _ => return None,
    };
    if let Some(ch) = draft[..start].chars().next_back() {
        if !matches!(ch, '\u{0009}'..='\u{000D}' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
        {
            return None;
        }
    }
    Some(Token {
        kind,
        start,
        query: &draft[query_start..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_tokens_preserve_unicode_prefixes_and_source_word_rules() {
        assert_eq!(
            parse("Hi🙂 @a-b"),
            Some(Token {
                kind: Kind::Source,
                start: 7,
                query: "a-b"
            })
        );
        assert_eq!(
            parse("/SUM_1"),
            Some(Token {
                kind: Kind::Command,
                start: 0,
                query: "SUM_1"
            })
        );
        assert!(parse("mail@example").is_none());
        assert!(parse("@é").is_none());
        assert!(parse("/sum ").is_none());
        assert!(parse("/a/b").is_none());
        assert!(parse("\u{0085}@x").is_none());
        assert_eq!(parse("\u{FEFF}@").unwrap().start, 3);
    }
}
