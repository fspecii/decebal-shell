//! Reviewed theme/photo recommendations, independent of applying a desktop theme.
use super::parse::{self, Detail, Wall};
use serde_json::Value;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Debug)]
pub struct Choice {
    pub mode: String,
    pub subject: String,
    pub rationale: String,
    pub provenance: String,
    pub wall: Wall,
}
struct Catalogue {
    images: BTreeMap<String, Value>,
    themes: BTreeMap<String, Value>,
}
fn catalogue() -> Result<&'static Catalogue, String> {
    static DATA: OnceLock<Result<Catalogue, String>> = OnceLock::new();
    DATA.get_or_init(|| {
        let v: Value = serde_json::from_str(include_str!("../../../data/wallhaven-curated.json")).map_err(|e| e.to_string())?;
        if v["format"] != 1 {
            return Err("Unsupported curated wallpaper catalogue".into());
        }
        let read = |key: &str| v[key].as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).ok_or_else(|| format!("Missing catalogue {key}"));
        Ok(Catalogue { images: read("images")?, themes: read("themes")? })
    })
    .as_ref()
    .map_err(Clone::clone)
}
fn text(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_owned()
}
pub fn known(id: &str) -> bool {
    catalogue().is_ok_and(|c| c.images.contains_key(id))
}
pub fn rejected_tags<'a>(tags: impl IntoIterator<Item = &'a str>) -> bool {
    tags.into_iter().any(|s| {
        let s = s.to_ascii_lowercase().replace(['_', '-'], " ");
        ["ai art", "ai generated", "midjourney", "stable diffusion", "dall e", "digital art", "illustration", "space art"].iter().any(|bad| s.contains(bad))
    })
}
pub fn validate_live(wall: &Wall, detail: &Detail) -> Result<(), String> {
    if wall.purity != "sfw" || detail.flagged_ai || detail.photography_excluded || rejected_tags(detail.tags.iter().map(String::as_str)) {
        return Err("This recommendation no longer passes its SFW/photography checks. Choose another picture.".into());
    }
    Ok(())
}
pub fn choices(theme: &str) -> Result<Vec<Choice>, String> {
    let c = catalogue()?;
    let Some(profile) = c.themes.get(theme) else { return Ok(vec![]) };
    let mut choices = vec![];
    for mode in ["dark", "light"] {
        let id = profile[mode]["image_id"].as_str().ok_or("Missing curated image ID")?;
        let image = c.images.get(id).ok_or("Missing curated photo")?;
        if !parse::valid_id(id) || image["purity"] != "sfw" || image["ai_review"]["flagged_ai"] != false || image["ai_review"]["returned_ai_boolean"] == true {
            return Err("Curated photo failed metadata checks".into());
        }
        let tags = image["tags"].as_array().ok_or("Missing photo tags")?;
        if tags.iter().any(|t| t["id"] == 133451) || rejected_tags(tags.iter().filter_map(|t| t["name"].as_str())) {
            return Err("Curated photo has excluded tags".into());
        }
        let page = text(image, "page_url");
        let thumb = text(image, "thumbnail_url");
        let full = text(image, "full_url");
        if page != format!("https://wallhaven.cc/w/{id}")
            || !["jpg", "png", "webp"].iter().any(|ext| thumb == format!("https://th.wallhaven.cc/lg/{}/{id}.{ext}", &id[..2]))
            || !["jpg", "png", "webp"].iter().any(|ext| full == format!("https://w.wallhaven.cc/full/{}/wallhaven-{id}.{ext}", &id[..2]))
        {
            return Err("Curated photo has unexpected source URLs".into());
        }
        let dims = image["resolution"].as_array().ok_or("Missing photo dimensions")?;
        let width = dims.first().and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or("Invalid photo width")?;
        let height = dims.get(1).and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or("Invalid photo height")?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 120_000_000 {
            return Err("Invalid photo size".into());
        }
        let author = image["author"]["name"].as_str().unwrap_or("unverified");
        let uploader = image["uploader"]["username"].as_str().unwrap_or("unknown");
        let license = image["license"]["identifier"].as_str().unwrap_or("unknown");
        let provenance = format!("Uploader: {uploader}. Author: {author}. Licence: {license}. Photo selected by visual/tag review; absence of AI tags is not a provenance guarantee.");
        let file_type = if full.ends_with(".png") {
            "image/png"
        } else if full.ends_with(".webp") {
            "image/webp"
        } else {
            "image/jpeg"
        };
        choices.push(Choice {
            mode: mode.into(),
            subject: text(image, "subject"),
            rationale: text(&profile[mode], "rationale"),
            provenance,
            wall: Wall {
                id: id.into(),
                url: page,
                thumb_small: thumb.clone(),
                thumb_large: thumb,
                full,
                resolution: format!("{width}x{height}"),
                width,
                height,
                file_size: 0,
                file_type: file_type.into(),
                purity: "sfw".into(),
                category: text(image, "category"),
                views: 0,
                favorites: 0,
                colors: vec![],
            },
        });
    }
    Ok(choices)
}

/// Read-only lookup of a previously downloaded recommendation. Never starts network work.
pub fn cached(theme: &str, dark: bool, backgrounds: &std::path::Path) -> Option<std::path::PathBuf> {
    let mode = if dark { "dark" } else { "light" };
    let choice = choices(theme).ok()?.into_iter().find(|c| c.mode == mode)?;
    ["jpg", "png", "webp"]
        .into_iter()
        .map(|ext| backgrounds.join(format!("wallhaven-{}.{ext}", choice.wall.id)))
        .find(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0 && m.len() <= super::client::MAX_IMAGE_BYTES))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shipped_recommendations_are_valid_and_custom_themes_stay_independent() {
        let c = catalogue().unwrap();
        for theme in c.themes.keys() {
            let picks = choices(theme).unwrap();
            assert_eq!(picks.len(), 2);
            assert_ne!(picks[0].wall.id, picks[1].wall.id);
            assert!(picks.iter().all(|p| p.provenance.contains("Author: unverified")));
        }
        assert!(choices("my-private-theme").unwrap().is_empty());
    }
    #[test]
    fn changed_provider_classification_is_refused() {
        let mut w = choices("macos").unwrap().remove(0).wall;
        assert!(validate_live(&w, &Detail::default()).is_ok());
        let d = Detail { tags: vec!["AI-generated".into()], ..Default::default() };
        assert!(validate_live(&w, &d).is_err());
        assert!(validate_live(&w, &Detail { flagged_ai: true, ..Default::default() }).is_err());
        w.purity = "nsfw".into();
        assert!(validate_live(&w, &Detail::default()).is_err());
    }
}
