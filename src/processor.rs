use content_inspector::{self, ContentType};
use sha1::Sha1;
use std::{fs::OpenOptions, io::Read, path::Path};

pub struct ParsedFile {
    hash: String,
    content_type: Option<ContentType>,
}

impl ParsedFile {
    pub fn new(hash: String, content_type: Option<ContentType>) -> Self {
        Self { hash, content_type }
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn content_type(&self) -> Option<ContentType> {
        self.content_type
    }
}

pub fn process(path: &Path) -> Option<ParsedFile> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(false)
        .create(false)
        .open(path)
        .ok()?;

    let mut buffer = [0u8; 1024 * 1024];
    let mut hash = Sha1::new();
    let mut content_type = None;

    loop {
        match file.read(&mut buffer) {
            Ok(0) => break Some(ParsedFile::new(hash.digest().to_string(), content_type)),
            Ok(n) => {
                if content_type.is_none() {
                    let len = if n > 1024 { 1024 } else { n };

                    content_type = Some(content_inspector::inspect(&buffer[0..len]));
                }

                hash.update(&buffer[0..n]);
            }
            Err(_) => break None,
        }
    }
}
