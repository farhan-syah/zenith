//! Font sourcing layer for Zenith.
//!
//! Provides a deterministic, system-font-free registry for resolving font bytes
//! by family name, weight, and style. All ordering-sensitive collections use
//! `BTreeMap` for determinism. No external crate dependencies — only `std`.

use std::collections::BTreeMap;
use std::sync::Arc;

/// The style variant of a font face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FontStyle {
    Normal,
    Italic,
}

/// Resolved font bytes ready for shaping or outlining. Cheap to clone (`Arc`).
#[derive(Clone)]
pub struct FontData {
    /// Stable identifier, e.g. `"noto-sans-400-normal"`.
    pub id: String,
    /// Raw font file bytes.
    pub bytes: Arc<[u8]>,
    /// Face index within a font collection (0 for single-face fonts).
    pub index: u32,
}

impl std::fmt::Debug for FontData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontData")
            .field("id", &self.id)
            .field("bytes_len", &self.bytes.len())
            .field("index", &self.index)
            .finish()
    }
}

/// Resolve font bytes by family + weight + style, or by stable id.
///
/// Implementations must never access system fonts.
pub trait FontProvider {
    /// Resolve by a priority-ordered family list, weight, and style.
    ///
    /// Iterates `families` in order. For each family:
    /// 1. Tries exact `(family, weight, style)`.
    /// 2. Falls back to the same family with any weight/style (first BTreeMap entry).
    ///
    /// Returns `None` only if no registered family matches any entry in `families`.
    /// Family comparison is case-insensitive.
    #[must_use]
    fn resolve(&self, families: &[String], weight: u16, style: FontStyle) -> Option<FontData>;

    /// Resolve by the stable id recorded on a shaped run.
    #[must_use]
    fn by_id(&self, id: &str) -> Option<FontData>;
}

// Internal key type for the primary registry map.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FaceKey {
    family_lower: String,
    weight: u16,
    style: FontStyle,
}

/// In-memory font registry. Register bundled and project fonts up front;
/// this implementation never scans the system.
///
/// Two `BTreeMap`s are maintained:
/// - `by_key`: `(family_lower, weight, style) -> FontData` for `resolve`.
/// - `by_id`: `id -> FontData` for `by_id`.
pub struct BytesFontProvider {
    by_key: BTreeMap<FaceKey, FontData>,
    by_id: BTreeMap<String, FontData>,
}

impl std::fmt::Debug for BytesFontProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<&str> = self.by_id.keys().map(String::as_str).collect();
        f.debug_struct("BytesFontProvider")
            .field("registered_faces", &ids)
            .finish()
    }
}

impl BytesFontProvider {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_key: BTreeMap::new(),
            by_id: BTreeMap::new(),
        }
    }

    /// Register a font face and return its stable id.
    ///
    /// The id is computed as `"{family_kebab_lower}-{weight}-{style_lower}"`,
    /// e.g. `"noto-sans-400-normal"`. If the same face is registered more than
    /// once, the most recent registration wins and reuses the original id.
    /// Because kebab-casing can collapse distinct families (e.g. `"My Font"`
    /// and `"my-font"`) onto the same base id, a numeric suffix is appended
    /// when the base id is already taken by a *different* face, so every
    /// registered face keeps a unique id. Returns the assigned stable id as a
    /// convenience; callers may register purely for the side effect.
    pub fn register(
        &mut self,
        family: &str,
        weight: u16,
        style: FontStyle,
        bytes: Arc<[u8]>,
        index: u32,
    ) -> String {
        let family_lower = family.to_lowercase();
        let family_kebab = family_lower.replace(' ', "-");
        let style_str = match style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
        };
        let base_id = format!("{family_kebab}-{weight}-{style_str}");

        let key = FaceKey {
            family_lower,
            weight,
            style,
        };

        // Re-registering the same face reuses its id; a new face whose base id
        // collides with a different face gets a numeric suffix.
        let id = match self.by_key.get(&key) {
            Some(existing) => existing.id.clone(),
            None => {
                let mut candidate = base_id.clone();
                let mut n = 2u32;
                while self.by_id.contains_key(&candidate) {
                    candidate = format!("{base_id}-{n}");
                    n += 1;
                }
                candidate
            }
        };

        let data = FontData {
            id: id.clone(),
            bytes,
            index,
        };

        self.by_key.insert(key, data.clone());
        self.by_id.insert(id.clone(), data);

        id
    }

    /// Return the lowercase family names of all registered faces (deduplicated, sorted).
    #[must_use]
    pub fn available_families(&self) -> Vec<String> {
        let mut families: Vec<String> =
            self.by_key.keys().map(|k| k.family_lower.clone()).collect();
        families.dedup();
        families
    }
}

impl Default for BytesFontProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FontProvider for BytesFontProvider {
    fn resolve(&self, families: &[String], weight: u16, style: FontStyle) -> Option<FontData> {
        for family in families {
            let family_lower = family.to_lowercase();

            // 1. Exact match.
            let exact_key = FaceKey {
                family_lower: family_lower.clone(),
                weight,
                style,
            };
            if let Some(data) = self.by_key.get(&exact_key) {
                return Some(data.clone());
            }

            // 2. Fallback: same family, any weight/style — deterministic first entry.
            let fallback = self
                .by_key
                .range(
                    FaceKey {
                        family_lower: family_lower.clone(),
                        weight: 0,
                        style: FontStyle::Normal,
                    }..,
                )
                .find(|(k, _)| k.family_lower == family_lower)
                .map(|(_, v)| v.clone());

            if fallback.is_some() {
                return fallback;
            }
        }
        None
    }

    fn by_id(&self, id: &str) -> Option<FontData> {
        self.by_id.get(id).cloned()
    }
}

/// Build a `BytesFontProvider` preloaded with the bundled default font.
///
/// The bundled font is Noto Sans Regular (Apache-2.0), embedded at compile time.
/// It is registered as family `"Noto Sans"`, weight `400`, style `Normal`.
#[must_use]
pub fn default_provider() -> BytesFontProvider {
    let bytes: Arc<[u8]> =
        Arc::from(&include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf")[..]);
    let mut provider = BytesFontProvider::new();
    provider.register("Noto Sans", 400, FontStyle::Normal, bytes, 0);
    provider
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: the four TrueType/OpenType magic bytes at offset 0.
    fn is_valid_tt_header(bytes: &[u8]) -> bool {
        bytes.len() > 1000 && bytes.starts_with(&[0x00, 0x01, 0x00, 0x00])
    }

    #[test]
    fn default_provider_resolves_noto_sans() {
        let p = default_provider();
        let result = p.resolve(&["Noto Sans".to_string()], 400, FontStyle::Normal);
        assert!(result.is_some(), "expected Some for Noto Sans 400 Normal");
        let data = result.unwrap();
        assert!(
            is_valid_tt_header(&data.bytes),
            "expected TrueType header and len > 1000, got len={}",
            data.bytes.len()
        );
    }

    #[test]
    fn case_insensitive_family_lookup() {
        let p = default_provider();
        let lower = p.resolve(&["noto sans".to_string()], 400, FontStyle::Normal);
        let mixed = p.resolve(&["Noto Sans".to_string()], 400, FontStyle::Normal);
        assert!(lower.is_some(), "lowercase family should resolve");
        assert!(mixed.is_some(), "mixed-case family should resolve");
        assert_eq!(lower.unwrap().id, mixed.unwrap().id);
    }

    #[test]
    fn weight_fallback_resolves_unregistered_weight() {
        let p = default_provider();
        // weight 700 is not registered — should fall back to the 400 face.
        let result = p.resolve(&["Noto Sans".to_string()], 700, FontStyle::Normal);
        assert!(
            result.is_some(),
            "weight 700 should fall back to registered 400 face"
        );
        let data = result.unwrap();
        assert!(data.id.contains("noto-sans"), "id should contain noto-sans");
    }

    #[test]
    fn unknown_family_returns_none() {
        let p = default_provider();
        let result = p.resolve(&["Nonexistent".to_string()], 400, FontStyle::Normal);
        assert!(result.is_none(), "unknown family must return None");
    }

    #[test]
    fn by_id_roundtrip() {
        let p = default_provider();
        let resolved = p
            .resolve(&["Noto Sans".to_string()], 400, FontStyle::Normal)
            .expect("should resolve");
        let by_id = p
            .by_id(&resolved.id)
            .expect("by_id should return same face");
        assert_eq!(resolved.id, by_id.id);
        assert_eq!(resolved.bytes.len(), by_id.bytes.len());
    }

    #[test]
    fn by_id_unknown_returns_none() {
        let p = default_provider();
        assert!(p.by_id("no-such-font-0-normal").is_none());
    }

    #[test]
    fn manual_register_and_resolve() {
        let mut p = BytesFontProvider::new();
        let dummy_bytes: Arc<[u8]> = Arc::from(vec![0u8; 64].as_slice());
        let id = p.register(
            "Test Family",
            400,
            FontStyle::Normal,
            dummy_bytes.clone(),
            0,
        );
        assert_eq!(id, "test-family-400-normal");

        let resolved = p.resolve(&["Test Family".to_string()], 400, FontStyle::Normal);
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().id, "test-family-400-normal");
    }

    #[test]
    fn stable_id_format() {
        let mut p = BytesFontProvider::new();
        let bytes: Arc<[u8]> = Arc::from(vec![0u8; 4].as_slice());
        let id = p.register("My Font", 700, FontStyle::Italic, bytes, 0);
        assert_eq!(id, "my-font-700-italic");
    }

    #[test]
    fn re_registering_same_face_reuses_id() {
        let mut p = BytesFontProvider::new();
        let bytes: Arc<[u8]> = Arc::from(vec![1u8; 8].as_slice());
        let id1 = p.register("Inter", 400, FontStyle::Normal, bytes.clone(), 0);
        let id2 = p.register("Inter", 400, FontStyle::Normal, bytes, 0);
        assert_eq!(id1, id2, "same face re-registration keeps a stable id");
    }

    #[test]
    fn kebab_colliding_families_get_distinct_ids() {
        // "My Font" and "my-font" both kebab to "my-font-400-normal"; the second
        // must get a distinct id so it remains independently resolvable by id.
        let mut p = BytesFontProvider::new();
        let a: Arc<[u8]> = Arc::from(vec![0xAAu8; 4].as_slice());
        let b: Arc<[u8]> = Arc::from(vec![0xBBu8; 4].as_slice());
        let id_a = p.register("My Font", 400, FontStyle::Normal, a, 0);
        let id_b = p.register("my-font", 400, FontStyle::Normal, b, 0);
        assert_eq!(id_a, "my-font-400-normal");
        assert_ne!(id_a, id_b, "colliding families must not share an id");
        // Both remain resolvable by their distinct ids, with their own bytes.
        assert_eq!(p.by_id(&id_a).unwrap().bytes[0], 0xAA);
        assert_eq!(p.by_id(&id_b).unwrap().bytes[0], 0xBB);
    }
}
