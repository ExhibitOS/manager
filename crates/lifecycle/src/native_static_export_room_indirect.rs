// SPDX-License-Identifier: Apache-2.0
//! Verifies the optional indirect-light receipt against the committed file manifest.
use super::{Manifest, ManifestFile, Result, bad, digest, hash_valid};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const META_NAME: &str = "exhibitos-static-room-indirect";
const VIEWER_META_NAME: &str = "exhibitos-viewer-build-id";
#[cfg(test)]
const META_PREFIX: &str = "meta name=\"exhibitos-static-room-indirect\" content=\"";
const FORMAT: &str = "exhibitos-static-room-indirect-v1";
const RECIPE: &str = "room-diffuse-one-bounce-rgba8-v1";
const LEGACY_RECIPE_SHA256: &str = "598b2c4a92b70a031760a7ca950067de64afaacd9bf84ed91315a42d97a8b03a";
pub(crate) const ORIGINAL_VIEWER_BUILD_ID: &str =
    "f62384be916179e278514c828e3af21d2aa161b4d9f44ef4125191b6ecba0fe8";
const BUDGET: u64 = 1024 * 1024;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum RoomIndirect {
    #[serde(rename = "room-diffuse-one-bounce-rgba8-v1")]
    DiffuseOneBounceRgba8V1,
}

/// Authoritative expectations from the admitted runtime, never from the export.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RoomIndirectBinding {
    recipe_sha256: String,
    viewer_build_id: String,
}

impl RoomIndirectBinding {
    pub(crate) fn from_worker_rows(ordered_hashes: [&str; 2], viewer_build_id: &str) -> Self {
        let encoded = serde_json::to_vec(&ordered_hashes).expect("string array serializes");
        Self {
            recipe_sha256: digest(&encoded),
            viewer_build_id: viewer_build_id.into(),
        }
    }

    pub(super) fn valid(&self) -> bool {
        hash_valid(&self.recipe_sha256) && hash_valid(&self.viewer_build_id)
    }

    pub(super) fn matches_persisted(&self, persisted: Option<&Self>) -> bool {
        persisted.map_or_else(
            || self.recipe_sha256 == LEGACY_RECIPE_SHA256 && self.viewer_build_id == ORIGINAL_VIEWER_BUILD_ID,
            |binding| self == binding,
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "status",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum IndirectBake {
    LiveFallback,
    Baked {
        descriptor_sha256: String,
        rooms: u64,
        texture_bytes: u64,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    format: String,
    source: Source,
    recipe: Recipe,
    rooms: Vec<Room>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Source {
    publication_id: String,
    revision_sha256: String,
    body_sha256: String,
    index_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    id: String,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Room {
    room_id: String,
    charts: Vec<Chart>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Chart {
    surface_id: String,
    width: u64,
    height: u64,
    texture: Texture,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Texture {
    sha256: String,
    bytes: u64,
}
#[derive(Deserialize)]
struct Index {
    publication: IndexPublication,
}
#[derive(Deserialize)]
struct IndexPublication {
    path: String,
    sha256: String,
    bytes: u64,
}
#[derive(Deserialize)]
struct Body {
    publication: Publication,
    exhibition: Exhibition,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Publication {
    id: String,
    revision_sha256: String,
}
#[derive(Deserialize)]
struct Exhibition {
    rooms: Vec<SceneRoom>,
    surfaces: Vec<Surface>,
}
#[derive(Deserialize)]
struct SceneRoom {
    id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Surface {
    id: String,
    room_id: String,
    #[serde(rename = "type")]
    kind: String,
    dimensions: Dimensions,
}
#[derive(Deserialize)]
struct Dimensions {
    width: f64,
    height: f64,
}

pub(super) fn verify(
    mode: Option<RoomIndirect>,
    binding: Option<&RoomIndirectBinding>,
    private_editor: bool,
    manifest: &Manifest,
    html: &str,
    index: &[u8],
    mut read: impl FnMut(&str) -> Result<Vec<u8>>,
) -> Result<Option<IndirectBake>> {
    let descriptor = head_pin(html, META_NAME)?;
    if mode.is_none() {
        return if descriptor.is_some() || binding.is_some() {
            Err(bad())
        } else {
            Ok(None)
        };
    }
    if private_editor || binding.is_some_and(|binding| !binding.valid()) {
        return Err(bad());
    }
    let (recipe_sha, viewer_build_id) = binding.map_or(
        (LEGACY_RECIPE_SHA256, ORIGINAL_VIEWER_BUILD_ID),
        |binding| (binding.recipe_sha256.as_str(), binding.viewer_build_id.as_str()),
    );
    if head_pin(html, VIEWER_META_NAME)? != Some(viewer_build_id) {
        return Err(bad());
    }
    let Some(descriptor_sha256) = descriptor else {
        return Ok(Some(IndirectBake::LiveFallback));
    };

    // The caller verifies each manifest entry's bytes. These reads additionally
    // bind the receipt's references to that exact manifest and body.
    let files: BTreeMap<_, _> = manifest
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let descriptor: Descriptor = serde_json::from_slice(&read_entry(
        &files,
        &format!("data/{descriptor_sha256}.json"),
        descriptor_sha256,
        &mut read,
    )?)
    .map_err(|_| bad())?;
    let index: Index = serde_json::from_slice(index).map_err(|_| bad())?;
    let source = descriptor.source;
    if descriptor.format != FORMAT
        || descriptor.recipe.id != RECIPE
        || descriptor.recipe.sha256 != recipe_sha
        || source.publication_id != manifest.publication_id
        || source.revision_sha256 != manifest.revision_sha256
        || source.index_sha256 != manifest.index_sha256
        || source.body_sha256 != index.publication.sha256
        || index.publication.path != format!("data/{}.json", source.body_sha256)
        || !hash_valid(&source.body_sha256)
    {
        return Err(bad());
    }
    let body = read_entry(
        &files,
        &index.publication.path,
        &source.body_sha256,
        &mut read,
    )?;
    if body.len() as u64 != index.publication.bytes {
        return Err(bad());
    }
    let body: Body = serde_json::from_slice(&body).map_err(|_| bad())?;
    if body.publication.id != manifest.publication_id
        || body.publication.revision_sha256 != manifest.revision_sha256
        || descriptor.rooms.is_empty()
        || descriptor.rooms.len() > body.exhibition.rooms.len()
    {
        return Err(bad());
    }

    let declared_rooms: BTreeSet<_> = body
        .exhibition
        .rooms
        .iter()
        .map(|room| room.id.as_str())
        .collect();
    let mut seen_rooms = BTreeSet::new();
    let mut seen_tiles = BTreeSet::new();
    let mut texture_bytes = 0;
    for room in &descriptor.rooms {
        if !declared_rooms.contains(room.room_id.as_str())
            || !seen_rooms.insert(room.room_id.as_str())
        {
            return Err(bad());
        }
        let surfaces: BTreeMap<_, _> = body
            .exhibition
            .surfaces
            .iter()
            .filter(|surface| surface.room_id == room.room_id)
            .map(|surface| (surface.id.as_str(), surface))
            .collect();
        if surfaces.len() != 6 || room.charts.len() != surfaces.len() {
            return Err(bad());
        }
        let mut seen_surfaces = BTreeSet::new();
        for chart in &room.charts {
            let surface = surfaces.get(chart.surface_id.as_str()).ok_or_else(bad)?;
            if !seen_surfaces.insert(chart.surface_id.as_str())
                || !matches!(surface.kind.as_str(), "floor" | "ceiling" | "wall")
                || !chart_size_matches(chart.width, surface.dimensions.width)
                || !chart_size_matches(chart.height, surface.dimensions.height)
                || chart.texture.bytes != chart.width * chart.height * 4
                || !hash_valid(&chart.texture.sha256)
            {
                return Err(bad());
            }
            texture_bytes += chart.texture.bytes;
            if texture_bytes > BUDGET {
                return Err(bad());
            }
            let texture_path = format!("data/{}.bin", chart.texture.sha256);
            if files
                .get(texture_path.as_str())
                .is_none_or(|file| file.bytes != chart.texture.bytes)
            {
                return Err(bad());
            }
            if seen_tiles.insert(chart.texture.sha256.as_str()) {
                let data = read_entry(&files, &texture_path, &chart.texture.sha256, &mut read)?;
                if data.len() as u64 != chart.texture.bytes
                    || data.chunks_exact(4).any(|pixel| pixel[3] != 255)
                {
                    return Err(bad());
                }
            }
        }
    }
    Ok(Some(IndirectBake::Baked {
        descriptor_sha256: descriptor_sha256.into(),
        rooms: descriptor.rooms.len() as u64,
        texture_bytes,
    }))
}

// The tracked static shell emits an explicit head and one canonical meta tag.
// Only inspect named meta elements there; title and cover copy are display text.
fn head_pin<'a>(html: &'a str, name: &str) -> Result<Option<&'a str>> {
    let Some((mut head, _)) = html
        .split_once("<head>")
        .and_then(|(_, tail)| tail.split_once("</head>"))
    else {
        return Ok(None);
    };
    let prefix = format!("meta name=\"{name}\" content=\"");
    let mut pin = None;
    while let Some((_, after)) = head.split_once('<') {
        if let Some(comment) = after.strip_prefix("!--") {
            head = comment.split_once("-->").map_or("", |(_, rest)| rest);
            continue;
        }
        head = after;
        let element = after.split('<').next().unwrap_or_default();
        if let Some(name) = ["title", "script", "style"]
            .into_iter()
            .find(|name| starts_with_tag(element, name))
        {
            head = after
                .split_once(&format!("</{name}>"))
                .map_or("", |(_, rest)| rest);
            continue;
        }
        if !is_named_meta(element, name) {
            continue;
        }
        let (digest, _) = element
            .strip_prefix(prefix.as_str())
            .and_then(|tail| tail.split_once("\">"))
            .ok_or_else(bad)?;
        if !hash_valid(digest) || pin.replace(digest).is_some() {
            return Err(bad());
        }
    }
    Ok(pin)
}

fn starts_with_tag(element: &str, name: &str) -> bool {
    element
        .get(..name.len())
        .is_some_and(|tag| tag.eq_ignore_ascii_case(name))
        && element
            .as_bytes()
            .get(name.len())
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))
}

fn is_named_meta(element: &str, meta_name: &str) -> bool {
    if !starts_with_tag(element, "meta") {
        return false;
    }
    let mut attrs = &element[4..];
    loop {
        attrs = attrs.trim_start_matches(|c: char| c.is_ascii_whitespace());
        let end = attrs
            .find(|c: char| c.is_ascii_whitespace() || matches!(c, '=' | '>' | '/'))
            .unwrap_or(attrs.len());
        if end == 0 {
            return false;
        }
        let name = &attrs[..end];
        attrs = attrs[end..].trim_start_matches(|c: char| c.is_ascii_whitespace());
        let Some(value) = attrs.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start_matches(|c: char| c.is_ascii_whitespace());
        let (value, rest) = match value.as_bytes().first() {
            Some(b'\'' | b'"') => {
                let quote = value.as_bytes()[0] as char;
                value[1..].split_once(quote).unwrap_or((&value[1..], ""))
            }
            _ => {
                let end = value
                    .find(|c: char| c.is_ascii_whitespace() || c == '>')
                    .unwrap_or(value.len());
                value.split_at(end)
            }
        };
        if name.eq_ignore_ascii_case("name") && value == meta_name {
            return true;
        }
        attrs = rest;
    }
}

fn chart_size_matches(pixels: u64, metres: f64) -> bool {
    (1..=64).contains(&pixels) && (metres / 0.25).ceil() == pixels as f64
}

fn read_entry(
    files: &BTreeMap<&str, &ManifestFile>,
    path: &str,
    sha: &str,
    read: &mut impl FnMut(&str) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    let file = files.get(path).ok_or_else(bad)?;
    if file.sha256 != sha || file.bytes > BUDGET {
        return Err(bad());
    }
    let bytes = read(path)?;
    if bytes.len() as u64 != file.bytes || digest(&bytes) != sha {
        return Err(bad());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn derives_receipt_from_complete_owned_charts_and_exact_texture_bytes() {
        let publication_id = "synthetic-publication";
        let revision_sha = "b".repeat(64);
        let tile = vec![1, 2, 3, 255];
        let tile_sha = digest(&tile);
        let tile_path = format!("data/{tile_sha}.bin");
        let surfaces: Vec<_> = (0..6)
            .map(|i| {
                json!({
                    "id": format!("surface-{i}"), "roomId": "room", "type": "wall",
                    "dimensions": {"width": 0.25, "height": 0.25}
                })
            })
            .collect();
        let body = serde_json::to_vec(&json!({
            "publication": {"id": publication_id, "revisionSha256": revision_sha},
            "exhibition": {"rooms": [{"id": "room"}], "surfaces": surfaces}
        }))
        .unwrap();
        let body_sha = digest(&body);
        let body_path = format!("data/{body_sha}.json");
        let index = serde_json::to_vec(&json!({
            "publication": {"path": body_path, "sha256": body_sha, "bytes": body.len()}
        }))
        .unwrap();
        let index_sha = digest(&index);
        let charts: Vec<_> = (0..6)
            .map(|i| {
                json!({
                    "surfaceId": format!("surface-{i}"), "width": 1, "height": 1,
                    "texture": {"sha256": tile_sha, "bytes": 4}
                })
            })
            .collect();
        let mut descriptor = json!({
            "format": FORMAT,
            "source": {
                "publicationId": publication_id, "revisionSha256": revision_sha,
                "bodySha256": body_sha, "indexSha256": index_sha
            },
            "recipe": {"id": RECIPE, "sha256": LEGACY_RECIPE_SHA256},
            "rooms": [{"roomId": "room", "charts": charts}]
        });
        let attempt = |descriptor: &serde_json::Value,
                       html_suffix: &str,
                       tex: &[u8],
                       private_editor,
                       binding: Option<&RoomIndirectBinding>,
                       viewer_build_id: &str| {
            let descriptor = serde_json::to_vec(descriptor).unwrap();
            let descriptor_sha = digest(&descriptor);
            let files = BTreeMap::from([
                (body_path.clone(), body.clone()),
                (tile_path.clone(), tile.clone()),
                (format!("data/{descriptor_sha}.json"), descriptor.clone()),
            ]);
            let manifest = Manifest {
                format: "exhibitos-static-publication-v1".into(),
                publication_id: publication_id.into(),
                revision_sha256: revision_sha.clone(),
                index_sha256: index_sha.clone(),
                files: files
                    .iter()
                    .map(|(path, bytes)| ManifestFile {
                        path: path.clone(),
                        bytes: bytes.len() as u64,
                        sha256: digest(bytes),
                    })
                    .collect(),
            };
            let html = format!(
                "<head><meta name=\"{VIEWER_META_NAME}\" content=\"{viewer_build_id}\"><title>{META_NAME}</title><{META_PREFIX}{descriptor_sha}\">{html_suffix}</head><body>{META_NAME}</body>"
            );
            verify(
                Some(RoomIndirect::DiffuseOneBounceRgba8V1),
                binding,
                private_editor,
                &manifest,
                &html,
                &index,
                |name| {
                    if name == tile_path {
                        Ok(tex.to_vec())
                    } else {
                        files.get(name).cloned().ok_or_else(bad)
                    }
                },
            )
        };
        let baked = attempt(&descriptor, "", &tile, false, None, ORIGINAL_VIEWER_BUILD_ID).unwrap().unwrap();
        assert_eq!(
            baked,
            IndirectBake::Baked {
                descriptor_sha256: digest(&serde_json::to_vec(&descriptor).unwrap()),
                rooms: 1,
                texture_bytes: 24,
            }
        );
        assert_eq!(serde_json::to_value(baked).unwrap()["textureBytes"], 24);
        assert!(
            attempt(
                &descriptor,
                "<meta name=\"exhibitos-static-room-indirect\">",
                &tile,
                false,
                None,
                ORIGINAL_VIEWER_BUILD_ID
            )
            .is_err()
        );
        assert!(attempt(&descriptor, "", &[1, 2, 3, 0], false, None, ORIGINAL_VIEWER_BUILD_ID).is_err());
        assert!(attempt(&descriptor, "", &tile, true, None, ORIGINAL_VIEWER_BUILD_ID).is_err());
        let viewer = "e".repeat(64);
        let worker_hashes = ["a".repeat(64), "b".repeat(64)];
        let successor = RoomIndirectBinding::from_worker_rows(
            [&worker_hashes[0], &worker_hashes[1]], &viewer,
        );
        assert!(attempt(&descriptor, "", &tile, false, Some(&successor), &viewer).is_err());
        let mut successor_descriptor = descriptor.clone();
        successor_descriptor["recipe"]["sha256"] = json!(successor.recipe_sha256);
        assert!(attempt(&successor_descriptor, "", &tile, false, Some(&successor), &viewer).is_ok());
        assert!(attempt(&successor_descriptor, "", &tile, false, Some(&successor), ORIGINAL_VIEWER_BUILD_ID).is_err());
        assert!(attempt(&successor_descriptor, "", &tile, false, None, ORIGINAL_VIEWER_BUILD_ID).is_err());
        descriptor["rooms"][0]["charts"][0]["surfaceId"] = json!("unowned-surface");
        assert!(attempt(&descriptor, "", &tile, false, None, ORIGINAL_VIEWER_BUILD_ID).is_err());
    }

    #[test]
    fn absent_meta_only_reports_fallback_for_an_explicit_viewer_request() {
        let manifest = Manifest {
            format: "exhibitos-static-publication-v1".into(),
            publication_id: "pub".into(),
            revision_sha256: "a".repeat(64),
            index_sha256: "b".repeat(64),
            files: vec![],
        };
        let mode = Some(RoomIndirect::DiffuseOneBounceRgba8V1);
        fn read(_: &str) -> Result<Vec<u8>> {
            panic!("fallback must not read a bake descriptor");
        }
        assert_eq!(verify(None, None, true, &manifest, "", b"", read).unwrap(), None);
        let old_head = format!("<head><meta name=\"{VIEWER_META_NAME}\" content=\"{ORIGINAL_VIEWER_BUILD_ID}\"></head>");
        assert!(verify(mode, None, false, &manifest, "", b"", read).is_err());
        assert_eq!(
            verify(mode, None, false, &manifest, &old_head, b"", read).unwrap(),
            Some(IndirectBake::LiveFallback)
        );
        assert!(verify(mode, None, true, &manifest, "", b"", read).is_err());
        assert_eq!(
            verify(
                None,
                None,
                false,
                &manifest,
                "exhibitos-static-room-indirect",
                b"",
                read
            )
            .unwrap(),
            None
        );
        let text = "<head><title>exhibitos-static-room-indirect</title><meta name=\"description\" content=\"exhibitos-static-room-indirect\"><!-- <meta name=\"exhibitos-static-room-indirect\"> --></head><body>&lt;meta name=&quot;exhibitos-static-room-indirect&quot;&gt;</body>";
        assert_eq!(
            verify(None, None, false, &manifest, text, b"", read).unwrap(),
            None
        );
        assert_eq!(
            verify(None, None, true, &manifest, text, b"", read).unwrap(),
            None
        );
        let text_with_viewer = text.replacen("<head>", old_head.trim_end_matches("</head>"), 1);
        assert_eq!(
            verify(mode, None, false, &manifest, &text_with_viewer, b"", read).unwrap(),
            Some(IndirectBake::LiveFallback)
        );
        let viewer = "e".repeat(64);
        let successor = RoomIndirectBinding::from_worker_rows([&"a".repeat(64), &"b".repeat(64)], &viewer);
        let successor_head = old_head.replace(ORIGINAL_VIEWER_BUILD_ID, &viewer);
        assert_eq!(
            verify(mode, Some(&successor), false, &manifest, &successor_head, b"", read).unwrap(),
            Some(IndirectBake::LiveFallback)
        );
        assert!(verify(mode, Some(&successor), false, &manifest, &old_head, b"", read).is_err());
        assert!(!successor.matches_persisted(None));
        assert!(successor.matches_persisted(Some(&successor)));
        let swapped = RoomIndirectBinding::from_worker_rows([&"b".repeat(64), &"a".repeat(64)], &viewer);
        assert!(!successor.matches_persisted(Some(&swapped)));
        let shape: RoomIndirectBinding = serde_json::from_value(serde_json::to_value(&successor).unwrap()).unwrap();
        assert_eq!(shape, successor);

        let bad_meta =
            "<head><META content='wrong' name = 'exhibitos-static-room-indirect'></head>";
        assert!(verify(None, None, false, &manifest, bad_meta, b"", read).is_err());
        assert!(verify(mode, None, false, &manifest, bad_meta, b"", read).is_err());
    }
}
