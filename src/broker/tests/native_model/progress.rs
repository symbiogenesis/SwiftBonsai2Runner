//! Bounded public-response observations survive cancellation of a device request.

use super::*;

const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
const EVENT_LIMIT: usize = 64 * 1024;

enum WireState {
    Headers,
    Size,
    Data(usize),
    End,
    Final,
    Done,
}

struct Progress {
    state: WireState,
    cursor: usize,
    event: Vec<u8>,
    fields: Value,
}

impl Progress {
    fn new(label: &str) -> Self {
        Self {
            state: WireState::Headers,
            cursor: 0,
            event: Vec::new(),
            fields: json!({"phase":label,"responseBytes":0,"decodedPayloadBytes":0,
                "completeSseRecords":0,"visibleContentEvents":0,"visibleContentBytes":0,
                "firstResponseMilliseconds":null,"headersMilliseconds":null,
                "firstVisibleContentMilliseconds":null,"lastVisibleContentMilliseconds":null,
                "terminalReasonObserved":null,"doneMarkerObserved":false,
                "finalChunkObserved":false,"connectionClosed":false,
                "reportedCompletionTokens":null,"requestMilliseconds":0,
                "headersComplete":false,"framingRejected":false,
                "cancelledBeforeConnectionClose":false,
                "emittedTokenCountKnown":false,"decodeTokensPerSecond":null,
                "scope":"client-observed public HTTP/SSE progress; event and byte counts are not generated-token counts; request/read timestamps include buffering, transport, observer parsing and synchronous receipt I/O"}),
        }
    }

    fn event_byte(&mut self, byte: u8, milliseconds: f64) {
        if self.event.len() == EVENT_LIMIT {
            self.fields["framingRejected"] = json!(true);
            return;
        }
        self.event.push(byte);
        if !self.event.ends_with(b"\n\n") && !self.event.ends_with(b"\r\n\r\n") {
            return;
        }
        let event = std::mem::take(&mut self.event);
        let Ok(text) = std::str::from_utf8(&event) else {
            self.fields["framingRejected"] = json!(true);
            return;
        };
        let mut data = text.lines().filter_map(|line| line.strip_prefix("data: "));
        let Some(data_line) = data.next() else { return };
        if data.next().is_some() {
            self.fields["framingRejected"] = json!(true);
            return;
        }
        self.increment("completeSseRecords", 1);
        if data_line == "[DONE]" {
            self.fields["doneMarkerObserved"] = json!(true);
            return;
        }
        let Ok(event) = serde_json::from_str::<Value>(data_line) else {
            self.fields["framingRejected"] = json!(true);
            return;
        };
        if let Some(tokens) = event["usage"]["completion_tokens"].as_u64() {
            self.fields["reportedCompletionTokens"] = json!(tokens);
        }
        if let Some(choices) = event["choices"].as_array() {
            for choice in choices {
                if let Some(text) = choice["delta"]["content"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                {
                    self.increment("visibleContentEvents", 1);
                    self.increment("visibleContentBytes", text.len());
                    if self.fields["firstVisibleContentMilliseconds"].is_null() {
                        self.fields["firstVisibleContentMilliseconds"] = json!(milliseconds);
                    }
                    self.fields["lastVisibleContentMilliseconds"] = json!(milliseconds);
                }
                if let Some(reason) = choice["finish_reason"].as_str() {
                    if ["length", "stop"].contains(&reason) {
                        self.fields["terminalReasonObserved"] = json!(reason);
                    } else {
                        self.fields["framingRejected"] = json!(true);
                    }
                }
            }
        }
    }

    fn increment(&mut self, field: &str, count: usize) {
        self.fields[field] =
            json!(self.fields[field].as_u64().unwrap() + u64::try_from(count).unwrap());
    }

    fn observe(&mut self, bytes: &[u8], milliseconds: f64) {
        self.fields["responseBytes"] = json!(bytes.len());
        self.fields["requestMilliseconds"] = json!(milliseconds);
        if !bytes.is_empty() && self.fields["firstResponseMilliseconds"].is_null() {
            self.fields["firstResponseMilliseconds"] = json!(milliseconds);
        }
        while self.cursor < bytes.len() && self.fields["framingRejected"] != true {
            match self.state {
                WireState::Headers => {
                    let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
                        self.fields["framingRejected"] = json!(bytes.len() > 16 * 1024);
                        return;
                    };
                    if end + 4 > 16 * 1024 {
                        self.fields["framingRejected"] = json!(true);
                        return;
                    }
                    let Ok(head) = std::str::from_utf8(&bytes[..end]) else {
                        self.fields["framingRejected"] = json!(true);
                        return;
                    };
                    self.fields["headersComplete"] = json!(true);
                    self.fields["headersMilliseconds"] = json!(milliseconds);
                    if head.lines().next() != Some("HTTP/1.1 200 OK")
                        || !head
                            .lines()
                            .any(|line| line.eq_ignore_ascii_case("Transfer-Encoding: chunked"))
                    {
                        self.fields["framingRejected"] = json!(true);
                        return;
                    }
                    self.cursor = end + 4;
                    self.state = WireState::Size;
                }
                WireState::Size => {
                    let remaining = &bytes[self.cursor..];
                    let Some(end) = remaining.windows(2).position(|part| part == b"\r\n") else {
                        self.fields["framingRejected"] = json!(remaining.len() > 32);
                        return;
                    };
                    let length = std::str::from_utf8(&remaining[..end])
                        .ok()
                        .and_then(|text| usize::from_str_radix(text, 16).ok());
                    let Some(length) =
                        length.filter(|length| *length <= RESPONSE_LIMIT && end <= 32)
                    else {
                        self.fields["framingRejected"] = json!(true);
                        return;
                    };
                    self.cursor += end + 2;
                    self.state = if length == 0 {
                        WireState::Final
                    } else {
                        WireState::Data(length)
                    };
                }
                WireState::Data(remaining) => {
                    let count = remaining.min(bytes.len() - self.cursor);
                    for byte in &bytes[self.cursor..self.cursor + count] {
                        self.event_byte(*byte, milliseconds);
                    }
                    self.increment("decodedPayloadBytes", count);
                    self.cursor += count;
                    self.state = if count == remaining {
                        WireState::End
                    } else {
                        WireState::Data(remaining - count)
                    };
                }
                WireState::End | WireState::Final => {
                    if bytes.len() - self.cursor < 2 {
                        return;
                    }
                    if &bytes[self.cursor..self.cursor + 2] != b"\r\n" {
                        self.fields["framingRejected"] = json!(true);
                        return;
                    }
                    self.cursor += 2;
                    if matches!(self.state, WireState::Final) {
                        self.fields["finalChunkObserved"] = json!(true);
                        self.state = WireState::Done;
                    } else {
                        self.state = WireState::Size;
                    }
                }
                WireState::Done => {
                    self.fields["framingRejected"] = json!(true);
                }
            }
        }
    }
}

struct Receipt<'a> {
    progress: Progress,
    started: tokio::time::Instant,
    evidence: &'a Path,
    report: &'a mut Value,
    index: usize,
}

impl Receipt<'_> {
    fn save(&mut self) {
        self.progress.fields["requestMilliseconds"] =
            json!(self.started.elapsed().as_secs_f64() * 1000.0);
        self.report["requestProgress"][self.index] = self.progress.fields.clone();
        save_acceptance_report(self.evidence, self.report);
    }
}

impl Drop for Receipt<'_> {
    fn drop(&mut self) {
        self.progress.fields["cancelledBeforeConnectionClose"] =
            json!(self.progress.fields["connectionClosed"] != true);
        self.progress.fields["requestMilliseconds"] =
            json!(self.started.elapsed().as_secs_f64() * 1000.0);
        self.report["requestProgress"][self.index] = self.progress.fields.clone();
        // Cancellation or a failed read may already be unwinding. Preserve the
        // observation when possible without introducing a second panic.
        if let Ok(bytes) = serde_json::to_vec_pretty(self.report) {
            let _ = fs::write(self.evidence.join("acceptance.json"), bytes);
        }
    }
}

#[expect(
    clippy::redundant_pub_crate,
    reason = "The parent and sibling acceptance modules share only this bounded observer."
)]
pub(super) async fn exchange(
    port: u16,
    credential: &str,
    body: &Value,
    evidence: &Path,
    report: &mut Value,
    label: &str,
) -> Vec<u8> {
    if report["requestProgress"].is_null() {
        report["requestProgress"] = json!([]);
    }
    let index = report["requestProgress"].as_array().unwrap().len();
    report["requestProgress"]
        .as_array_mut()
        .unwrap()
        .push(Value::Null);
    let mut receipt = Receipt {
        progress: Progress::new(label),
        started: tokio::time::Instant::now(),
        evidence,
        report,
        index,
    };
    receipt.save();
    let mut stream = connect(port, credential, body).await;
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = stream.read(&mut buffer).await.unwrap();
        if count == 0 {
            receipt.progress.fields["connectionClosed"] = json!(true);
            return bytes;
        }
        assert!(bytes.len() + count <= RESPONSE_LIMIT);
        bytes.extend_from_slice(&buffer[..count]);
        receipt
            .progress
            .observe(&bytes, receipt.started.elapsed().as_secs_f64() * 1000.0);
        receipt.save();
    }
}

#[test]
fn native_progress_decodes_fragmented_chunks_without_counting_events_as_tokens() {
    let payload = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"cedar\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"completion_tokens\":7}}\n\n",
        "data: [DONE]\n\n"
    );
    let mut wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    for chunk in payload.as_bytes().chunks(3) {
        wire.extend_from_slice(format!("{:X}\r\n", chunk.len()).as_bytes());
        wire.extend_from_slice(chunk);
        wire.extend_from_slice(b"\r\n");
    }
    wire.extend_from_slice(b"0\r\n\r\n");
    let mut progress = Progress::new("fixture");
    for end in 1..=wire.len() {
        progress.observe(&wire[..end], 2.0);
    }
    let fields = progress.fields;
    assert_eq!(fields["visibleContentEvents"], 1);
    assert_eq!(fields["visibleContentBytes"], 5);
    assert_eq!(fields["reportedCompletionTokens"], 7);
    assert_eq!(fields["completeSseRecords"], 3);
    assert_eq!(fields["terminalReasonObserved"], "stop");
    assert_eq!(fields["doneMarkerObserved"], true);
    assert_eq!(fields["finalChunkObserved"], true);
    assert_eq!(fields["connectionClosed"], false);
    assert_eq!(fields["emittedTokenCountKnown"], false);
    assert!(fields["decodeTokensPerSecond"].is_null());
    let mut rejected = Progress::new("fixture");
    rejected.observe(
        b"HTTP/1.1 500 Error\r\nTransfer-Encoding: chunked\r\n\r\n",
        1.0,
    );
    assert_eq!(rejected.fields["framingRejected"], true);
    assert_eq!(rejected.fields["visibleContentBytes"], 0);
}

#[test]
fn native_progress_drop_retains_partial_content_without_qualifying_completion() {
    let evidence = env::temp_dir().join(format!(
        "Bonsai-Progress-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&evidence).unwrap();
    let mut report = json!({"passed":false,"streams":[],"requestProgress":[null]});
    {
        let mut receipt = Receipt {
            progress: Progress::new("required-scale-1.3-stream"),
            started: tokio::time::Instant::now(),
            evidence: &evidence,
            report: &mut report,
            index: 0,
        };
        let payload =
            "data: {\"choices\":[{\"delta\":{\"content\":\"cedar\"},\"finish_reason\":null}]}\n\n";
        // The content event is complete inside an unfinished HTTP chunk.
        let wire = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}",
            payload.len() + 100
        );
        receipt.progress.observe(wire.as_bytes(), 1.0);
    }
    let saved: Value =
        serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
    let progress = &saved["requestProgress"][0];
    assert_eq!(progress["cancelledBeforeConnectionClose"], true);
    assert_eq!(progress["visibleContentBytes"], 5);
    assert_eq!(progress["visibleContentEvents"], 1);
    assert_eq!(progress["finalChunkObserved"], false);
    assert_eq!(progress["doneMarkerObserved"], false);
    assert!(progress["reportedCompletionTokens"].is_null());
    assert_eq!(saved["passed"], false);
    assert_eq!(saved["streams"], json!([]));
    fs::remove_file(evidence.join("acceptance.json")).unwrap();
    fs::remove_dir(evidence).unwrap();
}
