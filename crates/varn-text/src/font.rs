use parking_lot::Mutex;
use std::sync::OnceLock;

struct Db {
    db: fontdb::Database,
}

static DB: OnceLock<Mutex<Db>> = OnceLock::new();

fn db() -> &'static Mutex<Db> {
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Mutex::new(Db { db })
    })
}

pub(crate) fn with_db<T>(f: impl FnOnce(&fontdb::Database) -> T) -> T {
    let g = db().lock();
    f(&g.db)
}

pub fn fonts() -> Vec<String> {
    let g = db().lock();
    let mut out: Vec<String> =
        g.db.faces()
            .filter_map(|f| {
                let (name, _) = f.families.first()?;
                Some(name.clone())
            })
            .collect();
    out.sort();
    out.dedup();
    out
}

pub(crate) fn sans_bytes() -> Result<(Vec<u8>, u32), String> {
    let id = with_db(|db| {
        db.query(&fontdb::Query {
            families: &[fontdb::Family::SansSerif],
            ..Default::default()
        })
    })
    .ok_or_else(|| "text: no system font".to_string())?;
    with_db(|db| db.with_face_data(id, |bytes, idx| (bytes.to_vec(), idx)))
        .ok_or_else(|| "text: font not loadable".to_string())
}
