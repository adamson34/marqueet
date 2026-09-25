//! Just enough `multipart/form-data` for the admin page's file uploads
//! (team logos and team packs), so uploads work with scripting off and
//! without another crate.

/// One field of a submitted form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    /// Set for file inputs (empty when no file was chosen).
    pub filename: Option<String>,
    pub data: Vec<u8>,
}

impl Field {
    /// The value as text (form fields, not files).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.data).into_owned()
    }
}

/// The boundary from a `Content-Type: multipart/form-data; boundary=...`.
pub fn boundary(content_type: &str) -> Option<String> {
    let (kind, params) = content_type.split_once(';')?;
    if !kind.trim().eq_ignore_ascii_case("multipart/form-data") {
        return None;
    }
    params.split(';').find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim().eq_ignore_ascii_case("boundary").then(|| v.trim().trim_matches('"').to_owned())
    })
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from > hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// A quoted parameter from a `Content-Disposition` line: `name="logo"`.
fn param(header: &str, key: &str) -> Option<String> {
    header.split(';').find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim().eq_ignore_ascii_case(key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// Splits a form body into its fields.
pub fn parse(body: &[u8], boundary: &str) -> Result<Vec<Field>, String> {
    let bad = || "that upload didn't arrive in one piece; try again".to_owned();
    let delim = format!("--{boundary}").into_bytes();
    let mut at = find(body, &delim, 0).ok_or_else(bad)? + delim.len();
    let mut fields = Vec::new();
    loop {
        // After a delimiter: "--" ends the form, CRLF starts a part.
        if body[at..].starts_with(b"--") {
            return Ok(fields);
        }
        if !body[at..].starts_with(b"\r\n") {
            return Err(bad());
        }
        let head_start = at + 2;
        let head_end = find(body, b"\r\n\r\n", head_start).ok_or_else(bad)?;
        let head = String::from_utf8_lossy(&body[head_start..head_end]).into_owned();
        let data_start = head_end + 4;
        let next = find(body, &[b"\r\n".as_slice(), &delim].concat(), data_start).ok_or_else(bad)?;
        let disposition =
            head.lines().find(|l| l.to_ascii_lowercase().starts_with("content-disposition:")).ok_or_else(bad)?;
        let name = param(disposition, "name").ok_or_else(bad)?;
        fields.push(Field { name, filename: param(disposition, "filename"), data: body[data_start..next].to_vec() });
        at = next + 2 + delim.len();
    }
}

/// The first field called `name`.
pub fn get<'a>(fields: &'a [Field], name: &str) -> Option<&'a Field> {
    fields.iter().find(|f| f.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &[u8] = b"--XyZ\r\n\
Content-Disposition: form-data; name=\"team\"\r\n\r\n\
espn:nfl:12\r\n\
--XyZ\r\n\
Content-Disposition: form-data; name=\"logo\"; filename=\"logo.png\"\r\n\
Content-Type: image/png\r\n\r\n\
\x89PNG\r\n--not-the-boundary\r\n\
--XyZ\r\n\
Content-Disposition: form-data; name=\"empty\"; filename=\"\"\r\n\r\n\
\r\n\
--XyZ--\r\n";

    #[test]
    fn finds_the_boundary() {
        assert_eq!(boundary("multipart/form-data; boundary=XyZ").as_deref(), Some("XyZ"));
        assert_eq!(boundary("multipart/form-data; charset=utf-8; boundary=\"a b\"").as_deref(), Some("a b"));
        assert_eq!(boundary("application/x-www-form-urlencoded"), None);
    }

    #[test]
    fn splits_text_and_file_fields() {
        let fields = parse(BODY, "XyZ").unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(get(&fields, "team").unwrap().text(), "espn:nfl:12");
        let logo = get(&fields, "logo").unwrap();
        assert_eq!(logo.filename.as_deref(), Some("logo.png"));
        assert_eq!(logo.data, b"\x89PNG\r\n--not-the-boundary", "binary data with CRLFs survives");
        let empty = get(&fields, "empty").unwrap();
        assert_eq!((empty.filename.as_deref(), empty.data.len()), (Some(""), 0));
    }

    #[test]
    fn parameters_match_exactly() {
        let line = "Content-Disposition: form-data; filename=\"a.png\"; name=\"logo\"";
        assert_eq!(param(line, "name").as_deref(), Some("logo"));
        assert_eq!(param(line, "filename").as_deref(), Some("a.png"));
    }

    #[test]
    fn truncated_bodies_are_refused() {
        assert!(parse(&BODY[..60], "XyZ").is_err());
        assert!(parse(b"nothing here", "XyZ").is_err());
        assert_eq!(parse(b"--XyZ--\r\n", "XyZ").unwrap(), vec![]);
    }
}
