use content_inspector::{self, ContentType};
use sha1::Sha1;
use std::{fs::OpenOptions, io::Read, path::Path, str};

pub struct ParsedFile {
    hash: String,
    content_type: Option<ContentType>,
    content: Vec<u8>,
}

impl ParsedFile {
    pub fn new(hash: String, content: Vec<u8>, content_type: Option<ContentType>) -> Self {
        Self {
            hash,
            content,
            content_type,
        }
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn content_type(&self) -> Option<ContentType> {
        self.content_type
    }

    pub fn str_content(&self) -> Option<&str> {
        match self.content_type? {
            ContentType::BINARY => None,
            ContentType::UTF_8 | ContentType::UTF_8_BOM => {
                Some(str::from_utf8(&self.content).unwrap_or(""))
            }
            ContentType::UTF_16LE | ContentType::UTF_16BE => {
                Some(str::from_utf8(&self.content).unwrap_or(""))
            }
            _ => panic!("I'll get to utf-32 later"),
        }
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
    let mut content = Vec::new();

    loop {
        match file.read(&mut buffer) {
            Ok(0) => {
                break Some(ParsedFile::new(
                    hash.digest().to_string(),
                    content,
                    content_type,
                ))
            }
            Ok(n) => {
                if content_type.is_none() {
                    let len = if n > 1024 { 1024 } else { n };

                    content_type = Some(content_inspector::inspect(&buffer[0..len]));
                }

                if content_type.unwrap() != ContentType::BINARY {
                    content.extend(&buffer[0..n]);
                }

                hash.update(&buffer[0..n]);
            }
            Err(_) => break None,
        }
    }
}
