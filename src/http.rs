use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub target: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
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
        }
    }
}

impl std::error::Error for ParseError {}

pub fn parse_request(request: &str) -> Result<HttpRequest, ParseError> {
    if request.is_empty() {
        return Err(ParseError::EmptyRequest);
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

        if headers
            .iter()
            .any(|(existing, _): &(String, String)| existing.eq_ignore_ascii_case(name))
        {
            return Err(ParseError::DuplicateHeader(name.to_owned()));
        }

        headers.push((name.to_owned(), value.to_owned()));
    }

    Ok(HttpRequest {
        method: method.to_owned(),
        target: target.to_owned(),
        version: version.to_owned(),
        headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_line_and_headers() {
        let request = "GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
        let parsed = parse_request(request).unwrap();

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
        let error = parse_request("GET HTTP/1.1\r\n\r\n").unwrap_err();
        assert_eq!(error, ParseError::MissingTarget);
    }

    #[test]
    fn rejects_unsupported_version() {
        let error = parse_request("GET / HTTP/2.0\r\n\r\n").unwrap_err();
        assert_eq!(error, ParseError::UnsupportedVersion);
    }

    #[test]
    fn rejects_malformed_header() {
        let error = parse_request("GET / HTTP/1.1\r\nHost localhost\r\n\r\n").unwrap_err();
        assert_eq!(error, ParseError::InvalidHeader);
    }

    #[test]
    fn rejects_duplicate_headers_case_insensitively() {
        let error = parse_request(
            "GET / HTTP/1.1\r\nHost: localhost\r\nhOsT: example.com\r\n\r\n",
        )
        .unwrap_err();
        assert_eq!(error, ParseError::DuplicateHeader("hOsT".to_owned()));
    }
}
