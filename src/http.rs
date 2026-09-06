use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLine {
    pub method: String,
    pub target: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    EmptyRequest,
    MissingMethod,
    MissingTarget,
    MissingVersion,
    UnsupportedVersion,
    InvalidRequestLine,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyRequest => "empty HTTP request",
            Self::MissingMethod => "missing HTTP method",
            Self::MissingTarget => "missing request target",
            Self::MissingVersion => "missing HTTP version",
            Self::UnsupportedVersion => "unsupported HTTP version",
            Self::InvalidRequestLine => "invalid HTTP request line",
        };
        f.write_str(message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse_request_line(request: &str) -> Result<RequestLine, ParseError> {
    let line = request.lines().next().ok_or(ParseError::EmptyRequest)?.trim_end_matches('\r');

    if line.is_empty() {
        return Err(ParseError::EmptyRequest);
    }

    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or(ParseError::MissingMethod)?;
    let target = parts.next().ok_or(ParseError::MissingTarget)?;
    let version = parts.next().ok_or(ParseError::MissingVersion)?;

    if parts.next().is_some() {
        return Err(ParseError::InvalidRequestLine);
    }

    if version != "HTTP/1.1" {
        return Err(ParseError::UnsupportedVersion);
    }

    Ok(RequestLine {
        method: method.to_owned(),
        target: target.to_owned(),
        version: version.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http11_request_line() {
        let request = "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let parsed = parse_request_line(request).unwrap();

        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.target, "/health");
        assert_eq!(parsed.version, "HTTP/1.1");
    }

    #[test]
    fn rejects_missing_target() {
        let error = parse_request_line("GET HTTP/1.1\r\n").unwrap_err();
        assert_eq!(error, ParseError::MissingTarget);
    }

    #[test]
    fn rejects_unsupported_version() {
        let error = parse_request_line("GET / HTTP/2.0\r\n").unwrap_err();
        assert_eq!(error, ParseError::UnsupportedVersion);
    }

    #[test]
    fn rejects_extra_request_line_tokens() {
        let error = parse_request_line("GET / HTTP/1.1 unexpected\r\n").unwrap_err();
        assert_eq!(error, ParseError::InvalidRequestLine);
    }
}
