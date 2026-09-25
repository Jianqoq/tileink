//! Own the font inputs before any backend starts shaping or rasterizing text.
use cosmic_text::fontdb::{Database, Family, Source};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write, path::Path, sync::Arc};
use tileink::TextFontSystem;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub struct Snapshot {
    locale: String,
    database: Database,
    blobs: BTreeMap<String, Arc<Vec<u8>>>,
    pub manifest: serde_json::Value,
}
impl Snapshot {
    pub fn system() -> Result<Self> {
        Self::freeze(TextFontSystem::new())
    }

    fn freeze(font_system: TextFontSystem) -> Result<Self> {
        let (locale, source) = font_system.into_locale_and_db();
        if source.faces().next().is_none() {
            return Err("example font snapshot contains no font faces".into());
        }
        let mut database = Database::new();
        database.set_serif_family(source.family_name(&Family::Serif));
        database.set_sans_serif_family(source.family_name(&Family::SansSerif));
        database.set_cursive_family(source.family_name(&Family::Cursive));
        database.set_fantasy_family(source.family_name(&Family::Fantasy));
        database.set_monospace_family(source.family_name(&Family::Monospace));
        let mut blobs = BTreeMap::<String, Arc<Vec<u8>>>::new();
        let mut faces = Vec::new();
        for face in source.faces() {
            let bytes = source
                .with_face_data(face.id, |bytes, _| bytes.to_vec())
                .ok_or_else(|| format!("cannot capture font {}", face.post_script_name))?;
            let sha256 = digest(&bytes);
            let bytes = blobs
                .entry(sha256.clone())
                .or_insert_with(|| Arc::new(bytes))
                .clone();
            let mut captured = face.clone();
            captured.source = Source::Binary(bytes);
            // Preserve face order and collection index. Every route clones this
            // database; none reopens the mutable system files during rendering.
            database.push_face_info(captured);
            faces.push(serde_json::json!({
                "blob":format!("{sha256}.font"),"sha256":sha256,"index":face.index,
                "families":face.families.iter().map(|(name,language)|serde_json::json!({"name":name,"language":format!("{language:?}")})).collect::<Vec<_>>(),
                "post_script_name":face.post_script_name,"style":format!("{:?}",face.style),
                "weight":face.weight.0,"stretch":format!("{:?}",face.stretch),"monospaced":face.monospaced,
            }));
        }
        let families = [
            Family::Serif,
            Family::SansSerif,
            Family::Cursive,
            Family::Fantasy,
            Family::Monospace,
        ];
        let manifest = serde_json::json!({
            "locale":locale,"ordered_faces":faces,
            "generic_families":families.iter().map(|family|serde_json::json!({"generic":format!("{family:?}"),"family":source.family_name(family)})).collect::<Vec<_>>(),
        });
        Ok(Self {
            locale,
            database,
            blobs,
            manifest,
        })
    }
    pub fn font_system(&self) -> TextFontSystem {
        TextFontSystem::new_with_locale_and_db(self.locale.clone(), self.database.clone())
    }
    pub fn write(&self, directory: &Path) -> Result<()> {
        std::fs::create_dir(directory)?;
        for (hash, bytes) in &self.blobs {
            let mut file = std::fs::File::options()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("{hash}.font")))?;
            file.write_all(bytes)?;
        }
        let mut file = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(directory.join("manifest.json"))?;
        file.write_all(&serde_json::to_vec_pretty(&self.manifest)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_fonts_cannot_certify_text_examples() {
        use super::*;
        assert!(
            Snapshot::freeze(TextFontSystem::new_with_locale_and_db(
                "en-US".to_owned(),
                Database::new()
            ))
            .is_err()
        );
    }
    #[test]
    fn frozen_fonts_keep_face_order_bytes_locale_and_families() -> super::Result<()> {
        use super::*;
        let mut source = Database::new();
        source.load_fonts_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/fonts"));
        source.set_sans_serif_family("Noto Sans");
        source.set_monospace_family("DejaVu Sans Mono");
        assert!(source.faces().count() > 0);
        let names: Vec<_> = source
            .faces()
            .map(|face| face.post_script_name.clone())
            .collect();
        let snapshot = Snapshot::freeze(TextFontSystem::new_with_locale_and_db(
            "en-US".to_owned(),
            source,
        ))?;
        assert_eq!(
            snapshot
                .database
                .faces()
                .map(|face| face.post_script_name.clone())
                .collect::<Vec<_>>(),
            names
        );
        assert!(
            snapshot
                .database
                .faces()
                .all(|face| matches!(face.source, Source::Binary(_)))
        );
        for face in snapshot.database.faces() {
            let hash = snapshot
                .database
                .with_face_data(face.id, |bytes, _| digest(bytes))
                .unwrap();
            assert!(snapshot.blobs.contains_key(&hash));
        }
        let (locale, cloned) = snapshot.font_system().into_locale_and_db();
        assert_eq!(locale, "en-US");
        assert_eq!(cloned.family_name(&Family::SansSerif), "Noto Sans");
        assert_eq!(cloned.family_name(&Family::Monospace), "DejaVu Sans Mono");
        assert_eq!(cloned.faces().count(), names.len());
        Ok(())
    }
}
