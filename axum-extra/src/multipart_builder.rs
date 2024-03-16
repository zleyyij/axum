//! Generate forms to use in responses. You're probably looking for [MultipartForm].

use axum::response::{IntoResponse, Response};
use fastrand;
use http::{header, HeaderMap};

/// The `Content-Transfer-Encoding` setting for a part.
#[derive(Debug)]
pub enum TransferEncoding {
    /// If not specified, encoding defaults to UTF-8
    Default,
    /// If transferring raw binary data that is not guaranteed to be valid UTF-8.
    Binary,
}

/// Create multipart forms to be used in API responses.
/// This struct implements [IntoResponse], and so it can be returned from a handler like normal.
#[derive(Debug)]
pub struct MultipartForm {
    parts: Vec<Part>,
}

impl MultipartForm {
    /// Construct a new empty multipart form with no parts.
    pub fn new() -> Self {
        MultipartForm { parts: Vec::new() }
    }

    /// Initialize a new multipart form with the provided vector of parts.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use axum_extra::multipart_builder::{MultipartForm, Part};
    ///
    /// let parts: Vec<Part> = vec![Part::text("foo", "abc"), Part::text("bar", "def")];
    /// let form = MultipartForm::with_parts(parts);
    /// ```
    pub fn with_parts(parts: Vec<Part>) -> Self {
        MultipartForm { parts }
    }

    /// Add a new [Part] to the form
    ///
    /// # Examples
    ///
    /// ```rust
    /// use axum_extra::multipart_builder::{MultipartForm, Part};
    ///
    /// let mut form = MultipartForm::new();
    /// form
    ///     .part(Part::text("foo", "abc"))
    ///     .part(Part::text("other_field_name", "def"))
    ///     .part(Part::file("file", "file.txt", vec![0x68, 0x68, 0x20, 0x6d, 0x6f, 0x6d]));
    /// ```
    pub fn part(&mut self, part: Part) -> &mut Self {
        self.parts.push(part);
        self
    }
}

impl IntoResponse for MultipartForm {
    fn into_response(self) -> Response {
        // see RFC2388 for details
        let boundary = generate_boundary();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={}", boundary)
                .parse()
                .unwrap(),
        );
        let mut serialized_form: Vec<u8> = Vec::new();
        for part in self.parts {
            // for each part, the boundary is preceded by two dashes
            serialized_form.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            serialized_form.extend_from_slice(&part.serialize());
        }
        serialized_form.extend_from_slice(format!("--{}--", boundary).as_bytes());
        (headers, serialized_form).into_response()
    }
}

impl Default for MultipartForm {
    fn default() -> Self {
        Self::new()
    }
}

// Every part is expected to contain:
// - a [Content-Disposition](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Disposition
// header, where `Content-Disposition` is set to `form-data`, with a parameter of `name` that is set to
// the name of the field in the form. In the below example, the name of the field is `user`:
// ```
// Content-Disposition: form-data; name="user"
// ```
// If the field contains a file, then the `filename` parameter may be set to the name of the file.
// Handling for non-ascii field names is not done here, support for non-ascii characters may be encoded using
// methodology described in RFC 2047.
// - (optionally) a `Content-Type` header, which if not set, defaults to `text/plain`.
// If the field contains a file, then the file should be identified with that file's MIME type (eg: `image/gif`).
// If the `MIME` type is not known or specified, then the MIME type should be set to `application/octet-stream`.
// - If the part does not conform to the default encoding, then the `Content-Transfer-Encoding` header may be supplied.
// Valid settings for that header are: "base64", "quoted-printable", "8bit", "7bit", and "binary".
/// A single part of a multipart form as defined by
/// <https://www.w3.org/TR/html401/interact/forms.html#h-17.13.4>
/// and RFC2388.
#[derive(Debug)]
pub struct Part {
    /// The name of the part in question
    name: String,
    /// If the part should be treated as a file, the filename that should be attached that part
    filename: Option<String>,
    /// The `Content-Type` header. While not strictly required, it is always set here
    mime_type: String,
    /// The content/body of the part
    contents: Vec<u8>,
    /// The encoding that the contents should be encoded under
    encoding: TransferEncoding,
}

impl Part {
    /// Create a new part with `Content-Type` of `text/plain` with the supplied name and contents.
    /// This form will not have a defined file name.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use axum_extra::multipart_builder::{MultipartForm, Part};
    ///
    /// // create a form with a single part that has a field with a name of "foo",
    /// // and a value of "abc"
    /// let parts: Vec<Part> = vec![Part::text("foo", "abc")];
    /// let form = MultipartForm::with_parts(parts);
    /// ```
    pub fn text(name: &str, contents: &str) -> Self {
        Self {
            name: name.to_owned(),
            filename: None,
            mime_type: "text/plain".to_owned(),
            contents: contents.as_bytes().to_vec(),
            encoding: TransferEncoding::Default,
        }
    }

    /// Create a new part containing a generic file, with a `Content-Type` of `application/octet-stream`
    /// using the provided file name, field name, and contents. If the MIME type of the file is known, consider
    /// using [Part::raw_part]. The contents of this part do not need to be valid UTF 8.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use axum_extra::multipart_builder::{MultipartForm, Part};
    ///
    /// // create a form with a single part that has a field with a name of "foo",
    /// // with a file name of "foo.txt", and with the specified contents
    /// let parts: Vec<Part> = vec![Part::file("foo", "foo.txt", vec![0x68, 0x68, 0x20, 0x6d, 0x6f, 0x6d])];
    /// let form = MultipartForm::with_parts(parts);
    /// ```
    pub fn file(field_name: &str, file_name: &str, contents: Vec<u8>) -> Self {
        Self {
            name: field_name.to_owned(),
            filename: Some(file_name.to_owned()),
            // If the `MIME` type is not known or specified, then the MIME type should be set to `application/octet-stream`.
            // See RFC2388 section 3 for specifics.
            mime_type: "application/octet-stream".to_owned(),
            contents,
            encoding: TransferEncoding::Binary,
        }
    }

    /// Create a new part with more fine-grained control over the semantics of that part. The caller
    /// is assumed to have set a valid MIME type.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use axum_extra::multipart_builder::{MultipartForm, Part};
    ///
    /// // create a form with a single part that has a field with a name of "part_name",
    /// // with a MIME type of "application/json", and the supplied contents. This part will not have an associated filename, but will be sent as binary, and does not
    /// // need to be valid UTF-8.
    /// let parts: Vec<Part> = vec![Part::raw_part("part_name", "application/json", vec![0x68, 0x68, 0x20, 0x6d, 0x6f, 0x6d]), None, TransferEncoding::Binary];
    /// let form = MultipartForm::with_parts(parts);
    /// ```
    pub fn raw_part(
        name: &str,
        mime_type: &str,
        contents: Vec<u8>,
        filename: Option<&str>,
        encoding: TransferEncoding,
    ) -> Self {
        Self {
            name: name.to_owned(),
            filename: filename.map(|f| f.to_owned()),
            mime_type: mime_type.to_owned(),
            contents,
            encoding,
        }
    }

    /// Serialize this part into a chunk that can be easily inserted into a larger form
    pub(super) fn serialize(&self) -> Vec<u8> {
        // A part is serialized in this general format:
        // // the filename is optional
        // Content-Disposition: form-data; name="FIELD_NAME"; filename="FILENAME"\r\n
        // // the mime type (not strictly required by the spec, but always sent here)
        // Content-Type: mime/type\r\n
        // // if the part does not conform to the rest of the request's encoding,
        // // this is specified
        // Content-Transfer-Encoding: "ENCODING"\r\n
        // // a blank line, then the contents of the file start
        // \r\n
        // CONTENTS\r\n

        // Format what we can as a string, then handle the rest at a byte level
        let mut serialized_part = format!("Content-Disposition: form-data; name=\"{}\"", self.name);
        // specify a filename if one was set
        if let Some(filename) = &self.filename {
            serialized_part += &format!("; filename=\"{}\"", filename);
        }
        serialized_part += "\r\n";
        // specify the MIME type
        serialized_part += &format!("Content-Type: {}\r\n", self.mime_type);
        // if an encoding was set, add that
        // determine what encoding to label the body of the field with
        let encoding: Option<&str> = match self.encoding {
            TransferEncoding::Default => None,
            TransferEncoding::Binary => Some("binary"),
        };
        if let Some(encoding) = encoding {
            serialized_part += &format!("Content-Transfer-Encoding: {}\r\n", encoding);
        }
        serialized_part += "\r\n";
        let mut part_bytes = serialized_part.as_bytes().to_vec();
        part_bytes.extend_from_slice(&self.contents);
        part_bytes.extend_from_slice(b"\r\n");

        part_bytes
    }
}

/// A boundary is defined as a user defined (arbitrary) value that does not occur in any of the data.
/// Because the specification does not clearly define a methodology for generating boundaries, this implementation
/// follow's Reqwest's, and generates a boundary in the format of `XXXXXXXX-XXXXXXXX-XXXXXXXX-XXXXXXXX` where `XXXXXXXX`
/// is a hexadecimal representation of a randomly generated u64.
fn generate_boundary() -> String {
    let a = fastrand::u64(..);
    let b = fastrand::u64(..);
    let c = fastrand::u64(..);
    let d = fastrand::u64(..);
    format!("{a:016x}-{b:016x}-{c:016x}-{d:016x}")
}

#[cfg(test)]
mod tests {
    use super::{MultipartForm, Part};
    use axum::{body::Body, http};
    use axum::{routing::get, Router};
    use http::{Request, Response};
    use http_body_util::BodyExt;
    // for `collect`
    use tower::ServiceExt; // for `call`, `oneshot`, and `ready`

    #[tokio::test]
    async fn process_form() -> Result<(), Box<dyn std::error::Error>> {
        // create a boilerplate handle that returns a form
        async fn handle() -> MultipartForm {
            let mut form = MultipartForm::new();
            form.part(Part::text("part1", "basictext"))
                .part(Part::file(
                    "part2",
                    "file.txt",
                    vec![0x68, 0x69, 0x20, 0x6d, 0x6f, 0x6d],
                ))
                .part(Part::raw_part(
                    "part3",
                    "text/plain",
                    b"rawpart".to_vec(),
                    None,
                    super::TransferEncoding::Default,
                ));
            form
        }

        // make a request to that handle
        let app = Router::new().route("/", get(handle));
        let response: Response<_> = app
            .oneshot(Request::builder().uri("/").body(Body::empty())?)
            .await?;
        // content_type header
        let ct_header = response.headers().get("content-type").unwrap().to_str()?;
        let boundary = ct_header.split("boundary=").nth(1).unwrap().to_owned();
        let body: &[u8] = &response.into_body().collect().await?.to_bytes();
        assert_eq!(
            std::str::from_utf8(body)?,
            &format!(
                "--{boundary}\r\n\
                Content-Disposition: form-data; name=\"part1\"\r\n\
                Content-Type: text/plain\r\n\
                \r\n\
                basictext\r\n\
                --{boundary}\r\n\
                Content-Disposition: form-data; name=\"part2\"; filename=\"file.txt\"\r\n\
                Content-Type: application/octet-stream\r\n\
                Content-Transfer-Encoding: binary\r\n\
                \r\n\
                hi mom\r\n\
                --{boundary}\r\n\
                Content-Disposition: form-data; name=\"part3\"\r\n\
                Content-Type: text/plain\r\n\
                \r\n\
                rawpart\r\n\
                --{boundary}--",
                boundary = boundary
            )
        );

        Ok(())
    }
}
