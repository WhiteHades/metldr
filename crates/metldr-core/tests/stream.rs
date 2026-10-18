use metldr_core::Decoder;

#[test]
fn decodes_bytewise_utf8_and_split_thinking_tags() {
    let input = " \r\n{\"message\":{\"content\":\"café <thi\"},\"done\":false}\n{\"message\":{\"content\":\"nk>private</think> tail\"},\"done\":true}\n \t";
    let mut decoder = Decoder::new();
    let mut output = String::new();
    for byte in input.as_bytes() {
        output.push_str(&decoder.push(&[*byte]).unwrap());
    }
    output.push_str(&decoder.finish().unwrap());
    assert_eq!(output, "café  tail");
}

#[test]
fn accepts_terminal_record_without_trailing_newline() {
    let mut decoder = Decoder::new();
    assert_eq!(
        decoder
            .push(br#"{"message":{"content":"final"},"done":true}"#)
            .unwrap(),
        ""
    );
    assert!(!decoder.is_done());
    assert_eq!(decoder.finish().unwrap(), "final");
    assert!(decoder.is_done());
    assert_eq!(decoder.finish().unwrap(), "");
}

#[test]
fn malformed_provider_truncated_and_oversized_streams_stay_failed() {
    for bad in [
        b"{broken}\n".as_slice(),
        b"{\"error\":\"provider detail\"}\n".as_slice(),
    ] {
        let mut decoder = Decoder::new();
        assert!(decoder.push(bad).is_err());
        assert!(decoder.finish().is_err());
    }

    let mut truncated = Decoder::new();
    truncated
        .push(br#"{"message":{"content":"partial"},"done":false}"#)
        .unwrap();
    assert!(truncated.finish().is_err());
    assert!(truncated.finish().is_err());

    let mut oversized = Decoder::new();
    assert!(oversized.push(&vec![b' '; 256 * 1024 + 1]).is_err());
    assert!(oversized.finish().is_err());
}
