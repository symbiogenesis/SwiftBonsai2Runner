use super::*;

const PCM: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";

fn chunked(body: &[u8]) -> Vec<u8> {
    [PCM, body].concat()
}

#[test]
fn bounded_audio_accepts_split_reads_without_losing_early_body_timing() -> crate::Result<()> {
    struct Split<'a>(&'a [u8]);
    impl Read for Split<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.0.read(&mut buffer[..1])
        }
    }
    let response = chunked(b"2\r\n\0\x80\r\n2\r\n\xff\x7f\r\n0\r\n\r\n");
    let decoded = decode(Split(&response), "audio/pcm", 4, Instant::now())?;
    assert_eq!(decoded.bytes, [0, 128, 255, 127]);
    assert!(decoded.first_body <= decoded.elapsed);
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: 4\r\nConnection: close\r\n\r\nwave";
    assert_eq!(
        decode(response.as_slice(), "audio/wav", 4, Instant::now())?.bytes,
        b"wave"
    );
    Ok(())
}

#[test]
fn chunked_audio_rejects_truncation_trailers_extensions_odd_chunks_and_overflow() {
    for body in [
        b"0\r\n\r\n".as_slice(),
        b"2\r\nx",
        b"2\r\nxx\r\n",
        b"1\r\nx\r\n0\r\n\r\n",
        b"2;foo=bar\r\nxx\r\n0\r\n\r\n",
        b"6\r\nxxxxxx\r\n0\r\n\r\n",
        b"2\r\nxx\r\n0\r\nX: a\r\n\r\n",
        b"2\r\nxx\r\n0\r\n\r\ntrailing",
        b"100000000\r\n",
    ] {
        assert!(decode(chunked(body).as_slice(), "audio/pcm", 4, Instant::now()).is_err());
    }
}

#[test]
fn response_headers_reject_redirects_duplicate_or_ambiguous_framing_and_large_heads() {
    for head in [
        "HTTP/1.1 302 Found\r\nLocation: http://example.invalid/\r\nContent-Length: 2\r\nContent-Type: audio/wav\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 2\r\nContent-Type: audio/wav\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 2\r\nContent-Type: audio/pcm\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: audio/wav\r\nContent-Encoding: gzip\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 200 OK\nContent-Length: 2\nContent-Type: audio/wav\nConnection: close\n\n",
    ] {
        assert!(
            decode(
                [head.as_bytes(), b"xx"].concat().as_slice(),
                "audio/wav",
                4,
                Instant::now()
            )
            .is_err()
        );
    }
    let huge = format!("HTTP/1.1 200 OK\r\nX-Large: {}", "x".repeat(MAX_HEAD));
    assert!(decode(huge.as_bytes(), "audio/wav", 4, Instant::now()).is_err());
    let head = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: audio/wav\r\nConnection: close\r\n\r\n";
    for body in [b"x".as_slice(), b"xxx"] {
        assert!(
            decode(
                [head.as_slice(), body].concat().as_slice(),
                "audio/wav",
                4,
                Instant::now()
            )
            .is_err()
        );
    }
}

#[test]
fn request_preflight_checks_caller_and_deadline_before_opening_a_socket() {
    let token = [b'a'; 64];
    let call = Call {
        port: 1,
        token: &token,
        body: None,
        media: "application/json",
        maximum: 64,
        deadline: Instant::now(),
    };
    assert!(exchange(&call, || Ok(())).is_err());
    let error = exchange(&call, || Err("Caller exited.".into())).err();
    assert_eq!(error.as_deref(), Some("Caller exited."));
    assert!(
        !error
            .unwrap_or_default()
            .contains(std::str::from_utf8(&token).unwrap_or_default())
    );
}

fn receive_request(stream: &mut TcpStream) -> crate::Result<Vec<u8>> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        if request.len() == MAX_HEAD {
            return Err("Synthetic request exceeded its bound.".into());
        }
        let mut byte = [0];
        stream
            .read_exact(&mut byte)
            .map_err(|error| error.to_string())?;
        request.push(byte[0]);
    }
    Ok(request)
}

#[test]
fn numeric_loopback_request_preserves_bearer_and_fixed_route() -> crate::Result<()> {
    let listener =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let server = std::thread::spawn(move || -> crate::Result<()> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let request = receive_request(&mut stream)?;
        let request = std::str::from_utf8(&request).map_err(|error| error.to_string())?;
        assert!(request.starts_with("GET /health HTTP/1.1\r\n"));
        assert!(request.contains(&format!("\r\nHost: 127.0.0.1:{port}\r\n")));
        assert!(request.contains(&format!("\r\nAuthorization: Bearer {}\r\n", "a".repeat(64))));
        assert!(request.contains("\r\nContent-Length: 0\r\n"));
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .map_err(|error| error.to_string())
    });
    let token = [b'a'; 64];
    let call = Call {
        port,
        token: &token,
        body: None,
        media: "application/json",
        maximum: 64,
        deadline: Instant::now() + Duration::from_secs(2),
    };
    let result = exchange(&call, || Ok(()));
    server
        .join()
        .map_err(|_| "Synthetic HTTP server panicked.")??;
    assert_eq!(result?.bytes, b"{}");
    Ok(())
}

#[test]
fn stalled_body_cannot_extend_the_original_request_deadline() -> crate::Result<()> {
    let listener =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let (finished, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || -> crate::Result<()> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        receive_request(&mut stream)?;
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: 4\r\nConnection: close\r\n\r\nx")
            .map_err(|error| error.to_string())?;
        let _ = wait.recv_timeout(Duration::from_secs(5));
        Ok(())
    });
    let token = [b'a'; 64];
    let call = Call {
        port,
        token: &token,
        body: None,
        media: "audio/wav",
        maximum: 4,
        deadline: Instant::now() + Duration::from_millis(250),
    };
    let started = Instant::now();
    let result = exchange(&call, || Ok(()));
    let elapsed = started.elapsed();
    let _ = finished.send(());
    server
        .join()
        .map_err(|_| "Synthetic HTTP server panicked.")??;
    assert!(result.is_err());
    assert!(elapsed < Duration::from_secs(2));
    Ok(())
}
