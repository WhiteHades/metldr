use super::Sse;

#[test]
fn framing_preserves_events_and_rejects_invalid_or_incomplete_input() {
    let mut parser = Sse::new("Messages");
    let wire = "\u{feff}: keepalive\r\nevent: answer\r\ndata: h\u{e9}\r\ndata: world\r\n\r\nevent: ignored\n\ndata:\n\n";
    let events: Vec<_> = wire
        .bytes()
        .filter_map(|byte| parser.byte(byte).transpose())
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        events,
        [
            ("answer".into(), "h\u{e9}\nworld".into()),
            ("".into(), "".into())
        ]
    );
    parser.finish().unwrap();

    let mut parser = Sse::with_bound("Responses", 8);
    for byte in b"data: 12\ndata: 34\n\n" {
        parser.byte(*byte).unwrap();
    }
    parser.finish().unwrap();

    let mut parser = Sse::with_bound("Chat", 4);
    for byte in b"data" {
        parser.byte(*byte).unwrap();
    }
    assert_eq!(
        parser.byte(b':').unwrap_err(),
        "Chat SSE line exceeds bound"
    );

    let mut parser = Sse::with_bound("Google", 8);
    for byte in b"data: 12\ndata: 34\ndata: 56" {
        parser.byte(*byte).unwrap();
    }
    assert_eq!(
        parser.byte(b'\n').unwrap_err(),
        "Google SSE event exceeds bound"
    );

    let mut parser = Sse::new("Messages");
    parser.byte(0xff).unwrap();
    assert_eq!(parser.byte(b'\n').unwrap_err(), "Messages SSE is not UTF8");

    for wire in ["data: pending", "data: pending\n", "event: pending\n"] {
        let mut parser = Sse::new("Responses");
        for byte in wire.bytes() {
            parser.byte(byte).unwrap();
        }
        assert_eq!(
            parser.finish().unwrap_err(),
            "Responses SSE ended within an event"
        );
    }
}
