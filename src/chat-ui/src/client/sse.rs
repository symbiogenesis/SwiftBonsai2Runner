//! Borrowed SSE framing and JSON deltas, separate from completed reply storage.

use super::{Completion, Failure, MAX_EVENT, Result};
use serde::{
    Deserialize, Deserializer,
    de::{IgnoredAny, SeqAccess, Visitor},
};
use std::{borrow::Cow, fmt, marker::PhantomData};

#[derive(Default)]
pub(super) struct Sse {
    framer: Framer,
    reply: Reply,
}

#[derive(Default)]
struct Reply {
    text: String,
    reasoning: String,
    completion: Option<Completion>,
    done: bool,
}

#[derive(Default)]
struct Framer {
    bom_bytes: u8,
    started: bool,
    after_cr: bool,
    line: Vec<u8>,
    data: Vec<u8>,
    event: Vec<u8>,
    dispatched: bool,
}

#[derive(Clone, Copy)]
struct Frame<'a> {
    data: &'a [u8],
    event: &'a [u8],
}

trait LendingFrames {
    type Frame<'a>
    where
        Self: 'a;

    fn next_frame<'a>(&'a mut self, bytes: &mut &[u8]) -> Result<Option<Self::Frame<'a>>>;
}

fn drain_frames<S: LendingFrames>(
    source: &mut S,
    mut bytes: &[u8],
    mut visit: impl for<'a> FnMut(S::Frame<'a>) -> Result<()>,
) -> Result<()> {
    while let Some(frame) = source.next_frame(&mut bytes)? {
        visit(frame)?;
    }
    Ok(())
}

// The transparent wrapper makes Serde's borrowed-Cow string visitor apply
// inside Option as well: unescaped strings borrow; JSON escapes own decoding.
#[derive(Deserialize)]
#[serde(transparent)]
struct Text<'a>(#[serde(borrow)] Cow<'a, str>);

#[derive(Deserialize)]
struct Chunk<'a> {
    #[serde(default, borrow)]
    choices: Choices<'a>,
    error: Option<IgnoredAny>,
}

#[derive(Default)]
enum Choices<'a> {
    #[default]
    Empty,
    One(Choice<'a>),
    Many,
}

impl<'de: 'a, 'a> Deserialize<'de> for Choices<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ChoicesVisitor<'a>(PhantomData<&'a ()>);

        impl<'de: 'a, 'a> Visitor<'de> for ChoicesVisitor<'a> {
            type Value = Choices<'a>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a sequence of completion choices")
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let Some(first) = seq.next_element()? else {
                    return Ok(Choices::Empty);
                };
                if seq.next_element::<IgnoredAny>()?.is_some() {
                    while seq.next_element::<IgnoredAny>()?.is_some() {}
                    return Ok(Choices::Many);
                }
                Ok(Choices::One(first))
            }
        }

        deserializer.deserialize_seq(ChoicesVisitor(PhantomData))
    }
}

#[derive(Deserialize)]
struct Choice<'a> {
    #[serde(default, borrow)]
    delta: Delta<'a>,
    index: Option<u32>,
    #[serde(borrow)]
    finish_reason: Option<Text<'a>>,
}

#[derive(Default, Deserialize)]
struct Delta<'a> {
    #[serde(borrow)]
    content: Option<Text<'a>>,
    #[serde(borrow)]
    reasoning_content: Option<Text<'a>>,
    #[serde(borrow)]
    reasoning: Option<Text<'a>>,
}

impl Sse {
    pub(super) fn push(
        &mut self,
        bytes: &[u8],
        delta: &mut impl FnMut(Cow<'_, str>, Cow<'_, str>) -> Result<()>,
    ) -> Result<()> {
        drain_frames(&mut self.framer, bytes, |frame| {
            self.reply.dispatch(frame, delta)
        })
    }

    pub(super) fn finish(&self) -> Result<Completion> {
        if !self.reply.done {
            return Err(Failure::Message(
                "The local model ended its reply without confirming that generation finished.",
            ));
        }
        if self.reply.text.is_empty() && self.reply.reasoning.is_empty() {
            return Err(Failure::Message("The local model returned no text."));
        }
        // SSE discards any final event lacking a blank-line terminator. The
        // explicit terminal event and [DONE] must both have dispatched.
        self.reply.completion.ok_or(Failure::Message(
            "The response omitted its terminal completion reason.",
        ))
    }

    pub(super) fn into_answer(self) -> (String, String) {
        (self.reply.text, self.reply.reasoning)
    }
}

impl LendingFrames for Framer {
    type Frame<'a> = Frame<'a>;

    fn next_frame<'a>(&'a mut self, bytes: &mut &[u8]) -> Result<Option<Self::Frame<'a>>> {
        self.next_event(bytes)
    }
}

impl Framer {
    // A frame borrows only framing storage. Consumers can validate, accumulate
    // or enqueue its text before the next call recycles that storage.
    fn next_event(&mut self, bytes: &mut &[u8]) -> Result<Option<Frame<'_>>> {
        if self.dispatched {
            self.data.clear();
            self.event.clear();
            self.dispatched = false;
        }
        while !bytes.is_empty() {
            if self.started && !self.after_cr {
                let plain_len = memchr::memchr2(b'\r', b'\n', bytes).unwrap_or(bytes.len());
                if plain_len > 0 {
                    if self.line.len() + plain_len > MAX_EVENT {
                        return Err(Failure::Message("A streaming line exceeded its limit."));
                    }
                    self.line.extend_from_slice(&bytes[..plain_len]);
                    *bytes = &bytes[plain_len..];
                    if bytes.is_empty() {
                        break;
                    }
                }
            }
            let Some((&byte, rest)) = bytes.split_first() else {
                break;
            };
            *bytes = rest;
            if !self.started {
                match (self.bom_bytes, byte) {
                    (0, 0xef) => {
                        self.bom_bytes = 1;
                        continue;
                    }
                    (1, 0xbb) => {
                        self.bom_bytes = 2;
                        continue;
                    }
                    (2, 0xbf) => {
                        self.started = true;
                        continue;
                    }
                    _ => {
                        self.started = true;
                        // A failed BOM prefix is ordinary line data. Its two
                        // possible prefix bytes cannot contain a line ending.
                        if self.bom_bytes > 0 {
                            self.line.push(0xef);
                        }
                        if self.bom_bytes > 1 {
                            self.line.push(0xbb);
                        }
                    }
                }
            }
            if self.push_byte(byte)? {
                self.dispatched = true;
                return Ok(Some(Frame {
                    data: &self.data,
                    event: &self.event,
                }));
            }
        }
        Ok(None)
    }

    fn push_byte(&mut self, byte: u8) -> Result<bool> {
        if self.after_cr {
            self.after_cr = false;
            if byte == b'\n' {
                return Ok(false);
            }
        }
        if matches!(byte, b'\r' | b'\n') {
            self.after_cr = byte == b'\r';
            if self.line.is_empty() {
                if self.data.is_empty() {
                    self.event.clear();
                } else {
                    self.data.pop();
                    return Ok(true);
                }
            } else {
                self.process_line()?;
                self.line.clear();
            }
        } else {
            if self.line.len() == MAX_EVENT {
                return Err(Failure::Message("A streaming line exceeded its limit."));
            }
            self.line.push(byte);
        }
        Ok(false)
    }

    fn process_line(&mut self) -> Result<()> {
        if self.line.first() == Some(&b':') {
            return Ok(());
        }
        let (field, value) = memchr::memchr(b':', &self.line).map_or(
            (self.line.as_slice(), b"".as_slice()),
            |colon| {
                let value = &self.line[colon + 1..];
                (
                    &self.line[..colon],
                    value.strip_prefix(b" ").unwrap_or(value),
                )
            },
        );
        match field {
            b"data" => {
                if self.data.len() + value.len() + 1 > MAX_EVENT {
                    return Err(Failure::Message("A streaming event exceeded its limit."));
                }
                self.data.extend_from_slice(value);
                self.data.push(b'\n');
            }
            b"event" => {
                self.event.clear();
                self.event.extend_from_slice(value);
            }
            // id/retry affect reconnection only. Requests are never replayed:
            // interruption cancels the owned broker generation.
            _ => {}
        }
        Ok(())
    }
}

impl Reply {
    fn dispatch(
        &mut self,
        frame: Frame<'_>,
        delta: &mut impl FnMut(Cow<'_, str>, Cow<'_, str>) -> Result<()>,
    ) -> Result<()> {
        if frame.event == b"error" {
            return Err(Failure::Message("The model reported a streaming error."));
        }
        if !frame.event.is_empty() && frame.event != b"message" {
            return Err(Failure::Message(
                "The model returned an unsupported event type.",
            ));
        }
        if frame.data == b"[DONE]" {
            if self.done || self.completion.is_none() {
                return Err(Failure::Message(
                    "The response had an invalid terminal completion marker.",
                ));
            }
            self.done = true;
            return Ok(());
        }
        if self.done {
            return Err(Failure::Message(
                "The response continued after its completion marker.",
            ));
        }
        // Ignored JSON fields need not materialize strings, so from_slice alone
        // can accept invalid UTF-8 there. Validate the complete event once;
        // from_str retains borrowing and skips redundant string UTF-8 checks.
        let data = std::str::from_utf8(frame.data)
            .map_err(|_| Failure::Message("The model returned an invalid streaming event."))?;
        let chunk: Chunk<'_> = serde_json::from_str(data)
            .map_err(|_| Failure::Message("The model returned an invalid streaming event."))?;
        if chunk.error.is_some() || matches!(chunk.choices, Choices::Many) {
            return Err(Failure::Message(
                "The model could not complete this response.",
            ));
        }
        if let Choices::One(choice) = chunk.choices {
            if self.completion.is_some() || choice.index.is_some_and(|index| index != 0) {
                return Err(Failure::Message(
                    "The model returned an unexpected completion choice.",
                ));
            }
            // Validate the complete event before publishing any of its text.
            // In particular, a filtered or unsupported terminal event must not
            // become a visible partial answer or recovered conversation context.
            let completion = match choice.finish_reason {
                None => None,
                Some(reason) => Some(match reason.0.as_ref() {
                    "stop" => Completion::Finished,
                    "length" => Completion::TokenLimit,
                    "content_filter" => {
                        return Err(Failure::Message("The model filtered this response."));
                    }
                    _ => {
                        return Err(Failure::Message(
                            "The model ended with an unsupported completion reason.",
                        ));
                    }
                }),
            };
            let text = choice
                .delta
                .content
                .map_or(Cow::Borrowed(""), |text| text.0);
            let reasoning = choice
                .delta
                .reasoning_content
                .or(choice.delta.reasoning)
                .map_or(Cow::Borrowed(""), |text| text.0);
            self.text.push_str(&text);
            self.reasoning.push_str(&reasoning);
            delta(text, reasoning)?;
            self.completion = completion;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_utf8_in_ignored_fields_cannot_publish_an_event() -> Result<()> {
        for template in [
            r#"{"ignored":"INVALID","choices":[{"delta":{"content":"visible"},"finish_reason":"stop"}]}"#,
            r#"{"choices":[{"ignored":{"nested":["INVALID"]},"delta":{"content":"visible"},"finish_reason":"stop"}]}"#,
            r#"{"choices":[{"delta":{"ignored":"INVALID","content":"visible"},"finish_reason":"stop"}]}"#,
        ] {
            let (prefix, suffix) = template
                .split_once("INVALID")
                .ok_or(Failure::Message("Missing invalid UTF-8 fixture marker"))?;
            for invalid in [b"\xff".as_slice(), b"\xc0\xaf", b"\xf0\x9f"] {
                let wire = [
                    b"data: ",
                    prefix.as_bytes(),
                    invalid,
                    suffix.as_bytes(),
                    b"\n\n",
                ]
                .concat();
                for fragment in [1, 2, 7, wire.len()] {
                    let mut sse = Sse::default();
                    let mut published = 0;
                    let result = wire.chunks(fragment).try_for_each(|bytes| {
                        sse.push(bytes, &mut |_, _| {
                            published += 1;
                            Ok(())
                        })
                    });
                    assert!(result.is_err());
                    assert_eq!(published, 0);
                    assert!(sse.finish().is_err());
                    assert_eq!(sse.into_answer(), (String::new(), String::new()));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn fragmented_unicode_events_preserve_exact_source_and_borrowing() -> Result<()> {
        let expected = "雨🌱e\u{301}👩\u{200d}🔬עברית\u{2028}";
        let wire = format!(
            "data: {{\"ignored\":\"非ASCII🌱\",\"choices\":[{{\"delta\":{{\"content\":\"{expected}\"}},\"finish_reason\":\"stop\"}}]}}\r\n\r\ndata: [DONE]\r\n\r\n"
        );
        for fragment in [1, 2, 3, 7, wire.len()] {
            let mut sse = Sse::default();
            let mut published = String::new();
            for bytes in wire.as_bytes().chunks(fragment) {
                sse.push(bytes, &mut |text, _| {
                    assert!(matches!(text, Cow::Borrowed(_)));
                    published.push_str(&text);
                    Ok(())
                })?;
            }
            assert_eq!(sse.finish()?, Completion::Finished);
            assert_eq!(published, expected);
            assert_eq!(sse.into_answer(), (expected.into(), String::new()));
        }
        Ok(())
    }

    #[test]
    fn terminal_payloads_are_validated_before_delivery() -> Result<()> {
        for (reason, expected) in [
            ("content_filter", None),
            ("tool_calls", None),
            ("unknown", None),
            ("stop", Some(Completion::Finished)),
            ("length", Some(Completion::TokenLimit)),
        ] {
            let mut sse = Sse::default();
            let mut delivered = Vec::new();
            sse.push(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"Prior answer\",\"reasoning_content\":\"Prior reasoning\"}}]}\n\n",
                &mut |text, reasoning| {
                    delivered.push((text.into_owned(), reasoning.into_owned()));
                    Ok(())
                },
            )?;
            let terminal = format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\" terminal answer\",\"reasoning_content\":\" terminal reasoning\"}},\"finish_reason\":\"{reason}\"}}]}}\n\n"
            );
            let result = sse.push(terminal.as_bytes(), &mut |text, reasoning| {
                delivered.push((text.into_owned(), reasoning.into_owned()));
                Ok(())
            });
            if let Some(completion) = expected {
                result?;
                sse.push(b"data: [DONE]\n\n", &mut |_, _| Ok(()))?;
                assert_eq!(sse.finish()?, completion);
                assert_eq!(delivered.len(), 2);
                assert_eq!(
                    sse.into_answer(),
                    (
                        "Prior answer terminal answer".into(),
                        "Prior reasoning terminal reasoning".into(),
                    )
                );
            } else {
                assert!(result.is_err(), "accepted unsupported reason {reason}");
                assert_eq!(
                    delivered,
                    [("Prior answer".into(), "Prior reasoning".into())]
                );
                assert!(sse.finish().is_err());
                assert!(sse.reply.completion.is_none());
                assert_eq!(
                    sse.into_answer(),
                    ("Prior answer".into(), "Prior reasoning".into())
                );
            }
        }

        let mut sse = Sse::default();
        assert!(
            sse.push(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"stop\"}]}\n\n",
                &mut |_, _| Err(Failure::Message("Synthetic cancelled delivery")),
            )
            .is_err()
        );
        assert!(sse.reply.completion.is_none());
        assert!(sse.finish().is_err());
        Ok(())
    }

    #[test]
    fn delta_callbacks_borrow_unescaped_text_and_own_escaped_text() -> Result<()> {
        let mut sse = Sse::default();
        let mut received = Vec::new();
        sse.push(
            concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Rain 雨\",\"reasoning\":\"Consider\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"\\n\\u96ea\",\"reasoning_content\":\"\\tthought\"},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            ).as_bytes(),
            &mut |text, reasoning| {
                if received.is_empty() {
                    assert!(matches!(text, Cow::Borrowed(_)));
                    assert!(matches!(reasoning, Cow::Borrowed(_)));
                } else {
                    assert!(matches!(text, Cow::Owned(_)));
                    assert!(matches!(reasoning, Cow::Owned(_)));
                }
                received.push((text.into_owned(), reasoning.into_owned()));
                Ok(())
            },
        )?;
        assert_eq!(sse.finish()?, Completion::Finished);
        assert_eq!(
            received,
            [
                ("Rain 雨".to_owned(), "Consider".to_owned()),
                ("\n雪".to_owned(), "\tthought".to_owned()),
            ]
        );
        assert_eq!(
            sse.into_answer(),
            ("Rain 雨\n雪".into(), "Consider\tthought".into())
        );
        Ok(())
    }

    #[test]
    fn framer_lends_one_event_and_retains_the_unconsumed_input() -> Result<()> {
        let mut framer = Framer::default();
        let mut bytes = b"event: message\r\ndata: first\r\n\r\ndata: second\n\n".as_slice();
        let frame = framer
            .next_event(&mut bytes)?
            .ok_or(Failure::Message("Missing first frame"))?;
        assert_eq!(frame.data, b"first");
        assert_eq!(frame.event, b"message");
        assert_eq!(bytes, b"\ndata: second\n\n");
        let frame = framer
            .next_event(&mut bytes)?
            .ok_or(Failure::Message("Missing second frame"))?;
        assert_eq!(frame.data, b"second");
        assert_eq!(frame.event, b"");
        assert!(framer.next_event(&mut bytes)?.is_none());
        Ok(())
    }

    #[test]
    fn failed_bom_prefix_stays_in_its_original_line() -> Result<()> {
        for prefix in [b"\xef".as_slice(), b"\xef\xbb"] {
            let mut framer = Framer::default();
            let wire = [prefix, b"data: ignored\n\ndata: accepted\n\n"].concat();
            let mut bytes = wire.as_slice();
            let frame = framer
                .next_event(&mut bytes)?
                .ok_or(Failure::Message("Missing post-prefix frame"))?;
            assert_eq!(frame.data, b"accepted");
            assert!(framer.next_event(&mut bytes)?.is_none());
        }
        Ok(())
    }
}
