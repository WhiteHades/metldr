use metldr_core::{Chunk, ContentType, chunk_for_embedding};

#[test]
fn normalizes_whitespace_and_preserves_short_text() {
    let chunks = chunk_for_embedding("  A\t  B\r\n\r\n\r\nC  ", ContentType::Article);
    assert_eq!(
        chunks,
        vec![Chunk {
            text: "A B\n\nC".to_owned(),
            index: 0,
            token_count: 2,
            start: 0,
            end: 6,
        }]
    );
    assert!(chunk_for_embedding("", ContentType::Article).is_empty());

    let trimmed = chunk_for_embedding(
        "\u{feff}\u{00a0}\u{0085}A\u{0085}\u{00a0}\u{feff}",
        ContentType::Article,
    );
    assert_eq!(trimmed[0].text, "\u{0085}A\u{0085}");
    assert_eq!((trimmed[0].start, trimmed[0].end), (0, 3));
}

#[test]
fn uses_content_type_windows_and_preserves_unicode_boundaries() {
    let ascii = "x".repeat(1700);
    for (length, content_type, ranges) in [
        (1500, ContentType::Article, vec![(0, 1200), (1000, 1500)]),
        (900, ContentType::Email, vec![(0, 800), (680, 900)]),
        (1700, ContentType::Pdf, vec![(0, 1600), (1360, 1700)]),
    ] {
        let text = &ascii[..length];
        let chunks = chunk_for_embedding(text, content_type);
        let expected = ranges
            .into_iter()
            .enumerate()
            .map(|(index, (start, end))| Chunk {
                text: text[start..end].to_owned(),
                index,
                token_count: (end - start).div_ceil(4),
                start,
                end,
            })
            .collect::<Vec<_>>();
        assert_eq!(chunks, expected);
    }

    let unicode = format!("{}🙂z", "x".repeat(1199));
    let chunks = chunk_for_embedding(&unicode, ContentType::Article);
    assert_eq!(
        chunks,
        vec![
            Chunk {
                text: "x".repeat(1199),
                index: 0,
                token_count: 300,
                start: 0,
                end: 1199,
            },
            Chunk {
                text: format!("{}🙂z", "x".repeat(200)),
                index: 1,
                token_count: 51,
                start: 999,
                end: 1202,
            },
        ]
    );
}
