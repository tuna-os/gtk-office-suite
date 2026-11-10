#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use suite_collab::codec::{decode_message, decode_request, encode_message, encode_request, Line, LineReader};

// The sidecar protocol (#1995): every line collabd writes reaches the app
// through `LineReader` and `decode_message`, and every line the app writes
// reaches collabd through `decode_request`. Messages carry what other
// collaborators sent, so this is network input, the same risk the document
// importers were fuzzed for.
//
// Beyond "does not panic": a line that decodes must survive its own
// encoder, or the two ends of the pipe disagree about what was said.
fuzz_target!(|data: &[u8]| {
    // A small limit, so the too-long path is reached inside a short run.
    let mut reader = LineReader::with_limit(Cursor::new(data), 4096);
    for _ in 0..data.len() + 1 {
        match reader.read_line() {
            Ok(Line::Text(line)) => {
                if let Ok(request) = decode_request(&line) {
                    let again = encode_request(request.req, &request.command);
                    assert_eq!(decode_request(&again).as_ref(), Ok(&request), "a request changed in its own round trip");
                }
                if let Ok(message) = decode_message(&line) {
                    let again = encode_message(message.req, &message.event);
                    assert_eq!(decode_message(&again).as_ref(), Ok(&message), "a message changed in its own round trip");
                }
            }
            Ok(Line::TooLong { .. }) | Ok(Line::NotUtf8) => {}
            Ok(Line::Eof) | Err(_) => break,
        }
    }
});
