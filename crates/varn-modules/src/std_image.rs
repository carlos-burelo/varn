use crate::artifact::{read_artifact, write_artifact, ArtifactClass, ArtifactKind};

pub struct ImageModule<'a> {
    pub id: &'a str,
    pub path: &'a str,
    pub pure: bool,
    pub interface: &'a [u8],
    pub bytecode: &'a [u8],
    pub source: &'a str,
}

pub struct StdImage {
    pub std_version: &'static str,
    pub host_api_version: u32,
    modules: Vec<ImageModule<'static>>,
}

const ENTRY_WORDS: usize = 11;
const FLAG_PURE: u32 = 1;

impl StdImage {
    pub fn parse(bytes: &'static [u8]) -> Result<Self, String> {
        let payload = read_artifact(ArtifactKind::StdBundle, bytes)
            .map_err(|e| format!("invalid std image: {e}"))?;
        let mut cursor = Cursor { payload, at: 0 };
        let host_api_version = cursor.word()?;
        let (version_off, version_len) = (cursor.word()?, cursor.word()?);
        let std_version = cursor.str_at(version_off, version_len)?;
        let count = cursor.word()? as usize;
        let mut modules = Vec::with_capacity(count);
        for _ in 0..count {
            let mut w = [0u32; ENTRY_WORDS];
            for slot in &mut w {
                *slot = cursor.word()?;
            }
            modules.push(ImageModule {
                id: cursor.str_at(w[0], w[1])?,
                path: cursor.str_at(w[2], w[3])?,
                pure: w[4] & FLAG_PURE != 0,
                interface: cursor.bytes_at(w[5], w[6])?,
                bytecode: cursor.bytes_at(w[7], w[8])?,
                source: cursor.str_at(w[9], w[10])?,
            });
        }
        if !modules.windows(2).all(|p| p[0].id < p[1].id) {
            return Err("std image module table is not sorted".to_owned());
        }
        Ok(Self {
            std_version,
            host_api_version,
            modules,
        })
    }

    pub fn module(&self, id: &str) -> Option<&ImageModule<'static>> {
        self.modules
            .binary_search_by(|m| m.id.cmp(id))
            .ok()
            .map(|i| &self.modules[i])
    }

    pub fn modules(&self) -> &[ImageModule<'static>] {
        &self.modules
    }
}

struct Cursor {
    payload: &'static [u8],
    at: usize,
}

impl Cursor {
    fn word(&mut self) -> Result<u32, String> {
        let end = self.at + 4;
        let raw = self
            .payload
            .get(self.at..end)
            .ok_or("std image truncated")?;
        self.at = end;
        Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    }

    fn bytes_at(&self, off: u32, len: u32) -> Result<&'static [u8], String> {
        let (off, len) = (off as usize, len as usize);
        self.payload
            .get(off..off + len)
            .ok_or_else(|| "std image section out of bounds".to_owned())
    }

    fn str_at(&self, off: u32, len: u32) -> Result<&'static str, String> {
        std::str::from_utf8(self.bytes_at(off, len)?)
            .map_err(|_| "std image string is not utf-8".to_owned())
    }
}

pub fn write_image(
    std_version: &str,
    host_api_version: u32,
    mut modules: Vec<ImageModule<'_>>,
) -> Vec<u8> {
    modules.sort_by(|a, b| a.id.cmp(b.id));
    let header_words = 4 + modules.len() * ENTRY_WORDS;
    let mut data: Vec<u8> = Vec::new();
    let base = (header_words * 4) as u32;
    let mut place = |bytes: &[u8]| -> [u32; 2] {
        let off = base + data.len() as u32;
        data.extend_from_slice(bytes);
        [off, bytes.len() as u32]
    };
    let version = place(std_version.as_bytes());
    let mut words: Vec<u32> = vec![
        host_api_version,
        version[0],
        version[1],
        modules.len() as u32,
    ];
    for m in &modules {
        let id = place(m.id.as_bytes());
        let path = place(m.path.as_bytes());
        let interface = place(m.interface);
        let bytecode = place(m.bytecode);
        let source = place(m.source.as_bytes());
        words.extend_from_slice(&id);
        words.extend_from_slice(&path);
        words.push(if m.pure { FLAG_PURE } else { 0 });
        words.extend_from_slice(&interface);
        words.extend_from_slice(&bytecode);
        words.extend_from_slice(&source);
    }
    let mut payload = Vec::with_capacity(words.len() * 4 + data.len());
    for w in words {
        payload.extend_from_slice(&w.to_le_bytes());
    }
    payload.extend_from_slice(&data);
    write_artifact(
        ArtifactKind::StdBundle,
        ArtifactClass::Distributable,
        &payload,
    )
}
