use std::fmt;

pub const MAX_HEADER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub target: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponseHead {
    pub version: String,
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: ResponseBodyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseBodyMode {
    None,
    ContentLength(usize),
    Chunked,
    UntilClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    EmptyRequest,
    MissingRequestLine,
    MissingMethod,
    MissingTarget,
    MissingVersion,
    InvalidRequestLine,
    UnsupportedVersion,
    InvalidHeader,
    DuplicateHeader(String),
    HeadersTooLarge,
    IncompleteRequest,
    MissingResponseStatus,
    InvalidStatusLine,
    InvalidStatusCode,
    InvalidContentLength,
    ConflictingContentLength,
    UnsupportedTransferEncoding,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequest => f.write_str("empty HTTP request"),
            Self::MissingRequestLine => f.write_str("missing HTTP request line"),
            Self::MissingMethod => f.write_str("missing HTTP method"),
            Self::MissingTarget => f.write_str("missing request target"),
            Self::MissingVersion => f.write_str("missing HTTP version"),
            Self::InvalidRequestLine => f.write_str("invalid HTTP request line"),
            Self::UnsupportedVersion => f.write_str("unsupported HTTP version"),
            Self::InvalidHeader => f.write_str("invalid HTTP header"),
            Self::DuplicateHeader(name) => write!(f, "duplicate HTTP header: {name}"),
            Self::HeadersTooLarge => f.write_str("HTTP headers exceed maximum size"),
            Self::IncompleteRequest => f.write_str("incomplete HTTP request headers"),
            Self::MissingResponseStatus => f.write_str("missing HTTP response status line"),
            Self::InvalidStatusLine => f.write_str("invalid HTTP response status line"),
            Self::InvalidStatusCode => f.write_str("invalid HTTP response status code"),
            Self::InvalidContentLength => f.write_str("invalid Content-Length header"),
            Self::ConflictingContentLength => f.write_str("conflicting Content-Length headers"),
            Self::UnsupportedTransferEncoding => f.write_str("unsupported Transfer-Encoding"),
        }
    }
}

impl std::error::Error for ParseError {}

pub fn parse_request(request: &str) -> Result<HttpRequest, ParseError> {
    if request.is_empty() {
        return Err(ParseError::EmptyRequest);
    }

    if request.len() > MAX_HEADER_BYTES {
        return Err(ParseError::HeadersTooLarge);
    }

    if !request.contains("\r\n\r\n") {
        return Err(ParseError::IncompleteRequest);
    }

    let mut lines = request.split("\r\n");
    let request_line = lines.next().ok_or(ParseError::MissingRequestLine)?;

    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(ParseError::MissingMethod)?;
    let target = parts.next().ok_or(ParseError::MissingTarget)?;
    let version = parts.next().ok_or(ParseError::MissingVersion)?;

    if parts.next().is_some() {
        return Err(ParseError::InvalidRequestLine);
    }

    if version != "HTTP/1.1" {
        return Err(ParseError::UnsupportedVersion);
    }

    let headers = parse_headers(&mut lines, true)?;

    Ok(HttpRequest {
        method: method.to_owned(),
        target: target.to_owned(),
        version: version.to_owned(),
        headers,
    })
}

pub fn parse_response_head(response: &[u8]) -> Result<HttpResponseHead, ParseError> {
    if response.is_empty() {
        return Err(ParseError::MissingResponseStatus);
    }

    if response.len() > MAX_HEADER_BYTES {
        return Err(ParseError::HeadersTooLarge);
    }

    let Some(header_end) = find_header_end(response) else {
        return Err(ParseError::IncompleteRequest);
    };

    let text =
        std::str::from_utf8(&response[..header_end]).map_err(|_| ParseError::InvalidStatusLine)?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().ok_or(ParseError::MissingResponseStatus)?;

    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    let status = parts.next().ok_or(ParseError::InvalidStatusLine)?;
    let reason = parts.next().unwrap_or_default();

    if version != "HTTP/1.0" && version != "HTTP/1.1" {
        return Err(ParseError::UnsupportedVersion);
    }

    if status.len() != 3 || !status.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseError::InvalidStatusCode);
    }

    let status = status
        .parse::<u16>()
        .map_err(|_| ParseError::InvalidStatusCode)?;
    let headers = parse_headers(&mut lines, false)?;
    let body = response_body_mode(status, version, &headers)?;

    Ok(HttpResponseHead {
        version: version.to_owned(),
        status,
        reason: reason.to_owned(),
        headers,
        body,
    })
}

fn parse_headers<'a, I>(
    lines: &mut I,
    reject_duplicates: bool,
) -> Result<Vec<(String, String)>, ParseError>
where
    I: Iterator<Item = &'a str>,
{
    let mut headers = Vec::new();

    for line in lines {
        if line.is_empty() {
            break;
        }

        let Some((name, value)) = line.split_once(':') else {
            return Err(ParseError::InvalidHeader);
        };

        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.contains('\r') || value.contains('\n') {
            return Err(ParseError::InvalidHeader);
        }

        if reject_duplicates
            && headers
                .iter()
                .any(|(existing, _): &(String, String)| existing.eq_ignore_ascii_case(name))
        {
            return Err(ParseError::DuplicateHeader(name.to_owned()));
        }

        headers.push((name.to_owned(), value.to_owned()));
    }

    Ok(headers)
}

fn response_body_mode(
    status: u16,
    version: &str,
    headers: &[(String, String)],
) -> Result<ResponseBodyMode, ParseError> {
    if (100..200).contains(&status) || status == 204 || status == 304 {
        return Ok(ResponseBodyMode::None);
    }

    let transfer_encoding = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
        .map(|(_, value)| value.as_str());

    if let Some(value) = transfer_encoding {
        let encodings: Vec<_> = value.split(',').map(str::trim).collect();
        if encodings
            .last()
            .is_some_and(|encoding| encoding.eq_ignore_ascii_case("chunked"))
        {
            return Ok(ResponseBodyMode::Chunked);
        }

        return Err(ParseError::UnsupportedTransferEncoding);
    }

    let content_lengths: Vec<usize> = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| {
            value
                .parse::<usize>()
                .map_err(|_| ParseError::InvalidContentLength)
        })
        .collect::<Result<_, _>>()?;

    if let Some(&length) = content_lengths.first() {
        if content_lengths.iter().any(|value| *value != length) {
            return Err(ParseError::ConflictingContentLength);
        }
        return Ok(ResponseBodyMode::ContentLength(length));
    }

    if version == "HTTP/1.0" {
        return Ok(ResponseBodyMode::UntilClose);
    }

    Ok(ResponseBodyMode::UntilClose)
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_request() -> &'static str {
        "GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    }

    #[test]
    fn parses_request_line_and_headers() {
        let parsed = parse_request(complete_request()).unwrap();

        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.target, "/health");
        assert_eq!(parsed.version, "HTTP/1.1");
        assert_eq!(
            parsed.headers,
            vec![
                ("Host".to_owned(), "localhost".to_owned()),
                ("Connection".to_owned(), "close".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_missing_target() {
        let error = parse_request("GET  \r\nHost: localhost\r\n\r\n").unwrap_err();
        assert_eq!(error, ParseError::MissingTarget);
    }

    #[test]
    fn rejects_unsupported_version() {
        let error = parse_request("GET / HTTP/2.0\r\nHost: localhost\r\n\r\n").unwrap_err();
        assert_eq!(error, ParseError::UnsupportedVersion);
    }

    #[test]
    fn rejects_malformed_header() {
        let error = parse_request("GET / HTTP/1.1\r\nHost localhost\r\n\r\n").unwrap_err();
        assert_eq!(error, ParseError::InvalidHeader);
    }

    #[test]
    fn rejects_duplicate_headers_case_insensitively() {
        let error = parse_request("GET / HTTP/1.1\r\nHost: localhost\r\nhOsT: example.com\r\n\r\n")
            .unwrap_err();
        assert_eq!(error, ParseError::DuplicateHeader("hOsT".to_owned()));
    }

    #[test]
    fn rejects_incomplete_headers() {
        let error = parse_request("GET / HTTP/1.1\r\nHost: localhost\r\n").unwrap_err();
        assert_eq!(error, ParseError::IncompleteRequest);
    }

    #[test]
    fn rejects_headers_over_limit() {
        let request = format!(
            "GET / HTTP/1.1\r\nHost: {}\r\n\r\n",
            "a".repeat(MAX_HEADER_BYTES)
        );
        let error = parse_request(&request).unwrap_err();
        assert_eq!(error, ParseError::HeadersTooLarge);
    }

    #[test]
    fn parses_content_length_response() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: keep-alive\r\n\r\n";
        let parsed = parse_response_head(response).unwrap();

        assert_eq!(parsed.status, 200);
        assert_eq!(parsed.reason, "OK");
        assert_eq!(parsed.body, ResponseBodyMode::ContentLength(5));
    }

    #[test]
    fn parses_http_10_response() {
        let response = b"HTTP/1.0 200 OK\r\nContent-Length: 5\r\n\r\n";
        let parsed = parse_response_head(response).unwrap();

        assert_eq!(parsed.version, "HTTP/1.0");
        assert_eq!(parsed.body, ResponseBodyMode::ContentLength(5));
    }

    #[test]
    fn parses_chunked_response() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        let parsed = parse_response_head(response).unwrap();

        assert_eq!(parsed.body, ResponseBodyMode::Chunked);
    }

    #[test]
    fn parses_no_body_status() {
        for status in [101, 204, 304] {
            let response = format!("HTTP/1.1 {status} Test\r\n\r\n");
            let parsed = parse_response_head(response.as_bytes()).unwrap();
            assert_eq!(parsed.body, ResponseBodyMode::None);
        }
    }

    #[test]
    fn defaults_to_until_close_without_framing_headers() {
        let response = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";
        let parsed = parse_response_head(response).unwrap();

        assert_eq!(parsed.body, ResponseBodyMode::UntilClose);
    }

    #[test]
    fn rejects_conflicting_content_lengths() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Length: 6\r\n\r\n";
        let error = parse_response_head(response).unwrap_err();

        assert_eq!(error, ParseError::ConflictingContentLength);
    }

    #[test]
    fn rejects_unsupported_transfer_encoding() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n";
        let error = parse_response_head(response).unwrap_err();

        assert_eq!(error, ParseError::UnsupportedTransferEncoding);
    }
}
