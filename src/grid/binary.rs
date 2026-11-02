//! Binary bytes with a presentation determined once, before drawing.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binary<'a> {
    data: Data<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Data<'a> {
    Text(&'a str),
    Hex(&'a [u8]),
}

impl<'a> Binary<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        let data = match std::str::from_utf8(bytes) {
            Ok(text) if !text.contains(char::is_control) => Data::Text(text),
            _ => Data::Hex(bytes),
        };
        Self { data }
    }

    /// Returns a value that a row holds as text.
    pub(super) fn of_text(text: &'a str) -> Self {
        let data = Data::Text(text);
        Self { data }
    }

    /// Returns a value that a row holds as bytes.
    pub(super) fn of_bytes(bytes: &'a [u8]) -> Self {
        let data = Data::Hex(bytes);
        Self { data }
    }

    pub fn text(&self) -> Option<&'a str> {
        match self.data {
            Data::Text(text) => Some(text),
            Data::Hex(_) => None,
        }
    }

    pub fn bytes(&self) -> &'a [u8] {
        match self.data {
            Data::Text(text) => text.as_bytes(),
            Data::Hex(bytes) => bytes,
        }
    }
}

impl std::ops::Deref for Binary<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.bytes()
    }
}
