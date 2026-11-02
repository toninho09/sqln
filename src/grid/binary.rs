//! Binary bytes with a presentation determined once, before drawing.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binary {
    data: Data,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Data {
    Text(Box<str>),
    Hex(Box<[u8]>),
}

impl Binary {
    pub fn new(bytes: Box<[u8]>) -> Self {
        let data = match String::from_utf8(bytes.into_vec()) {
            Ok(text) if !text.contains(char::is_control) => Data::Text(text.into_boxed_str()),
            Ok(text) => Data::Hex(text.into_bytes().into_boxed_slice()),
            Err(error) => Data::Hex(error.into_bytes().into_boxed_slice()),
        };
        Self { data }
    }

    pub fn text(&self) -> Option<&str> {
        match &self.data {
            Data::Text(text) => Some(text),
            Data::Hex(_) => None,
        }
    }
}

impl std::ops::Deref for Binary {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match &self.data {
            Data::Text(text) => text.as_bytes(),
            Data::Hex(bytes) => bytes,
        }
    }
}
