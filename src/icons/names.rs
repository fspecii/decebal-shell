//! Icon names: what to try, in order, for a dock id, a `.desktop` `Icon=` value or a MIME type name.
//!
//! Themes disagree on names (`user-trash` vs `trash`), apps are asked for by class (`org.mozilla.firefox`) and
//! MIME icons fall back from the specific to the generic one (`text-x-python` -> `text-x-generic`). The list is
//! ordered: the caller tries every name over the whole theme chain before moving to the next.

/// Dock / launcher ids that are not icon names themselves, mapped to the names themes use.
fn alias(id: &str) -> &'static [&'static str] {
    match id {
        "trash" => &["user-trash", "trash-empty", "trash"],
        "trash-full" => &["user-trash-full", "trash-full"],
        "finder" | "files" | "file-manager" => &["system-file-manager", "org.gnome.Nautilus", "folder"],
        "terminal" => &["utilities-terminal", "org.gnome.Terminal", "terminal"],
        "settings" | "system-settings" | "preferences" => &["preferences-system", "systemsettings", "org.gnome.Settings"],
        "browser" => &["web-browser", "internet-web-browser"],
        "mail" => &["mail-client", "internet-mail", "evolution"],
        "music" => &["audio-x-generic", "multimedia-audio-player", "folder-music"],
        "photos" | "image" | "images" => &["image-x-generic", "folder-pictures"],
        "video" | "videos" => &["video-x-generic", "folder-videos"],
        "calendar" => &["office-calendar", "x-office-calendar", "org.gnome.Calendar"],
        "calculator" => &["accessories-calculator", "org.gnome.Calculator"],
        "notes" | "text-editor" => &["accessories-text-editor", "text-editor", "text-x-generic"],
        "downloads" => &["folder-download", "folder-downloads"],
        "documents" => &["folder-documents", "folder"],
        "home" => &["user-home", "folder-home"],
        "desktop" => &["user-desktop", "folder-desktop"],
        "software" | "app-store" | "appstore" => &["system-software-install", "org.gnome.Software", "software-store"],
        "pdf" => &["application-pdf", "x-office-document"],
        "archive" => &["package-x-generic", "application-x-archive"],
        _ => &[],
    }
}

/// Prefixes of the Icon Naming Specification where dropping the last `-part` gives the more general icon
/// (`network-wired-disconnected` -> `network-wired`, `folder-pictures` -> `folder`).
const DASH_STRIP: &[&str] =
    &["network-", "audio-", "battery-", "weather-", "user-", "folder-", "input-", "media-", "display-", "preferences-", "security-", "dialog-", "emblem-", "notification-", "mail-", "go-", "view-"];

/// MIME-like icon names: the generic icon of their media type.
fn mime_generic(name: &str) -> Option<String> {
    let (ty, rest) = name.split_once('-')?;
    let known = matches!(ty, "text" | "image" | "audio" | "video" | "font" | "model" | "package" | "inode");
    if !known || rest.is_empty() || rest == "generic" || rest == "x-generic" {
        return None;
    }
    Some(match ty {
        "inode" => "folder".to_string(),
        _ => format!("{ty}-x-generic"),
    })
}

/// Icon names for `id`, best first, without duplicates. `generic` adds the last-resort application icons
/// (what the specification calls the default icon) for ids no theme knows.
pub fn candidates(id: &str, generic: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: &str| {
        if !s.is_empty() && !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    };
    let id = id.trim();
    add(id);
    let bare = id.strip_suffix(".desktop").unwrap_or(id);
    add(bare);
    add(&bare.to_lowercase());
    for a in alias(&bare.to_lowercase()) {
        add(a);
    }
    // reverse-DNS class names (org.mozilla.firefox, com.github.Foo.Bar): the last component, and the last two joined
    if bare.contains('.') && !bare.contains('/') && !bare.ends_with(['.']) {
        let parts: Vec<&str> = bare.split('.').collect();
        if parts.len() >= 3 {
            add(&parts[parts.len() - 1].to_lowercase());
            add(parts[parts.len() - 1]);
        }
    }
    // `-symbolic` icons fall back to the full colour one; the Icon Naming Specification's generalisation
    let lower = bare.to_lowercase();
    if let Some(base) = lower.strip_suffix("-symbolic") {
        add(base);
    }
    if DASH_STRIP.iter().any(|p| lower.starts_with(p)) {
        let mut cur = lower.trim_end_matches("-symbolic").to_string();
        while let Some((head, _)) = cur.rsplit_once('-') {
            if head.is_empty() {
                break;
            }
            cur = head.to_string();
            add(&cur);
        }
    }
    if let Some(g) = mime_generic(&lower) {
        add(&g);
    }
    // application/x-foo as a MIME name in dashes (application-x-foo): the generic executable icon
    if lower.starts_with("application-") {
        add("application-x-executable");
    }
    if generic {
        add("application-x-executable");
        add("application-default-icon");
        add("applications-other");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_itself_comes_first_then_lowercase() {
        assert_eq!(candidates("Firefox", false), ["Firefox", "firefox"]);
        assert_eq!(candidates("foot", false), ["foot"]);
        assert_eq!(candidates("firefox.desktop", false), ["firefox.desktop", "firefox"]);
    }

    #[test]
    fn dock_ids_map_to_theme_names() {
        assert_eq!(candidates("trash", false)[1], "user-trash");
        assert!(candidates("finder", false).contains(&"system-file-manager".to_string()));
        assert!(candidates("settings", false).contains(&"preferences-system".to_string()));
        assert!(candidates("Terminal", false).contains(&"utilities-terminal".to_string()), "aliases are matched case-insensitively");
    }

    #[test]
    fn reverse_dns_class_names_try_the_last_component() {
        let c = candidates("org.mozilla.firefox", false);
        assert_eq!(c[0], "org.mozilla.firefox");
        assert!(c.contains(&"firefox".to_string()));
        let c = candidates("com.github.Foo.Bar", false);
        assert!(c.contains(&"bar".to_string()) && c.contains(&"Bar".to_string()));
        assert_eq!(candidates("a.b", false), ["a.b"], "two components are not a reverse-DNS id");
        assert!(!candidates("/usr/share/x.png", false).iter().any(|c| c == "png"));
    }

    #[test]
    fn mime_names_fall_back_to_their_media_type() {
        let c = candidates("text-x-python", false);
        assert_eq!(c, ["text-x-python", "text-x-generic"]);
        assert!(candidates("image-png", false).contains(&"image-x-generic".to_string()));
        assert!(candidates("audio-x-flac", false).contains(&"audio-x-generic".to_string()));
        assert_eq!(candidates("text-x-generic", false), ["text-x-generic"], "the generic icon has no further fallback");
        let c = candidates("application-x-foo", false);
        assert_eq!(c.last().unwrap(), "application-x-executable");
    }

    #[test]
    fn naming_spec_names_drop_their_last_part() {
        let c = candidates("network-wired-disconnected", false);
        assert_eq!(c[..2], ["network-wired-disconnected", "network-wired"]);
        assert!(c.contains(&"network".to_string()));
        let c = candidates("folder-pictures", false);
        assert!(c.contains(&"folder".to_string()));
        let c = candidates("audio-volume-high-symbolic", false);
        assert!(c.contains(&"audio-volume-high".to_string()) && c.contains(&"audio-volume".to_string()));
        assert!(!candidates("visual-studio-code", false).contains(&"visual".to_string()), "app names are not generalised");
    }

    #[test]
    fn generic_adds_the_default_application_icons_last() {
        let c = candidates("nonexistent-app", true);
        assert_eq!(c, ["nonexistent-app", "application-x-executable", "application-default-icon", "applications-other"]);
        assert!(!candidates("nonexistent-app", false).contains(&"application-x-executable".to_string()));
        let again = candidates("application-x-foo", true);
        assert_eq!(again.iter().filter(|c| *c == "application-x-executable").count(), 1, "no duplicates");
    }
}
