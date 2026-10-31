//! The companion catalogues compiled into decebal: one line per desktop port, added when the port's
//! `assets/desktop-ports/<family>/asset-packs.toml` lands. The test below fails when a file in the repository is
//! missing here (or does not parse), so the list cannot drift.
//!
//! ```ignore
//! ("deepin", include_str!("../../assets/desktop-ports/deepin/asset-packs.toml")),
//! ```

pub const CATALOGS: &[(&str, &str)] =
    &[("gnome", include_str!("../../assets/desktop-ports/gnome/asset-packs.toml")), ("openkylin", include_str!("../../assets/desktop-ports/openkylin/asset-packs.toml"))];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_port_catalogue_in_the_repository_is_compiled_in_and_valid() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/desktop-ports");
        let mut on_disk: Vec<String> =
            std::fs::read_dir(&base).into_iter().flatten().flatten().filter(|e| e.path().join("asset-packs.toml").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        on_disk.sort();
        let mut listed: Vec<String> = CATALOGS.iter().map(|(f, _)| f.to_string()).collect();
        listed.sort();
        assert_eq!(on_disk, listed, "assets/desktop-ports/*/asset-packs.toml and builtin::CATALOGS differ");
        let mut set = crate::desktop_assets::catalog::Set::default();
        for (family, text) in CATALOGS {
            let c = crate::desktop_assets::catalog::parse(text, family).unwrap_or_else(|e| panic!("{}", e.join("\n")));
            assert_eq!(&c.family, family, "family key equals the folder name");
            set.push(c);
        }
        assert!(set.problems.is_empty(), "{:?}", set.problems);
    }
}
