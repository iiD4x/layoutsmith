//! Values from a project's `res/values*/` files, used to resolve references
//! such as `@color/primary`, `@dimen/margin` or `@string/title`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::units::{Color, parse_dimension};
use crate::xml::Document;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Style {
    pub parent: Option<String>,
    pub items: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct Resources {
    /// The `res` directory the values were loaded from.
    pub res_dir: Option<PathBuf>,
    pub colors: BTreeMap<String, String>,
    pub dimens: BTreeMap<String, String>,
    pub strings: BTreeMap<String, String>,
    pub styles: BTreeMap<String, Style>,
}

/// How deep `@color/a -> @color/b -> ...` chains are followed.
const MAX_INDIRECTION: usize = 16;

impl Resources {
    /// Loads the values next to a layout file: for `app/src/main/res/layout/x.xml`
    /// that is every `.xml` file in `app/src/main/res/values/`.
    pub fn for_layout(layout_path: &Path) -> Self {
        let res_dir = layout_path
            .parent()
            .filter(|dir| {
                dir.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n == "layout" || n.starts_with("layout-"))
            })
            .and_then(Path::parent);
        match res_dir {
            Some(dir) => Self::load(dir),
            None => Self::default(),
        }
    }

    /// Loads `values/*.xml` from a `res` directory. Unreadable or malformed
    /// files are skipped: a preview with a few unresolved colors is more useful
    /// than no preview.
    pub fn load(res_dir: &Path) -> Self {
        let mut res = Self {
            res_dir: Some(res_dir.to_path_buf()),
            ..Self::default()
        };
        let Ok(entries) = std::fs::read_dir(res_dir.join("values")) else {
            return res;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "xml"))
            .collect();
        files.sort();
        for file in files {
            if let Ok(text) = std::fs::read_to_string(&file) {
                res.add_values_xml(&text);
            }
        }
        res
    }

    /// Adds the entries of one values file.
    pub fn add_values_xml(&mut self, text: &str) {
        let Ok(doc) = Document::parse(text) else {
            return;
        };
        let Some(root) = doc.root() else { return };
        for node in doc.element_children(root) {
            let Some(name) = doc.attr(node, "name") else {
                continue;
            };
            let tag = match doc.name(node) {
                "item" => doc.attr(node, "type").unwrap_or_default(),
                t => t.to_string(),
            };
            match tag.as_str() {
                "color" => {
                    self.colors
                        .insert(name, doc.text_content(node).trim().to_string());
                }
                "dimen" => {
                    self.dimens
                        .insert(name, doc.text_content(node).trim().to_string());
                }
                "string" => {
                    self.strings
                        .insert(name, unescape_string(doc.text_content(node).trim()));
                }
                "style" => {
                    let items = doc
                        .element_children(node)
                        .into_iter()
                        .filter_map(|item| {
                            Some((
                                doc.attr(item, "name")?,
                                doc.text_content(item).trim().to_string(),
                            ))
                        })
                        .collect();
                    let parent = doc.attr(node, "parent").map(|p| {
                        p.trim_start_matches("@style/")
                            .trim_start_matches("@android:style/")
                            .to_string()
                    });
                    self.styles.insert(name, Style { parent, items });
                }
                _ => {}
            }
        }
    }

    /// Resolves a color literal, `@color/...`, `@android:color/...` or a theme
    /// attribute such as `?attr/colorPrimary`.
    pub fn resolve_color(&self, value: &str) -> Option<Color> {
        let mut value = value.trim().to_string();
        for _ in 0..MAX_INDIRECTION {
            if let Some(c) = Color::parse_hex(&value) {
                return Some(c);
            }
            if let Some(name) = value.strip_prefix("@android:color/") {
                return android_color(name);
            }
            value = match value.strip_prefix("@color/") {
                Some(name) => self.colors.get(name)?.clone(),
                None => self.theme_attr(theme_attr_name(&value)?)?,
            };
        }
        None
    }

    /// Resolves a dimension literal or `@dimen/...` to dp.
    pub fn resolve_dimension(&self, value: &str, density: f32) -> Option<f32> {
        let mut value = value.trim().to_string();
        for _ in 0..MAX_INDIRECTION {
            if let Some(v) = parse_dimension(&value, density) {
                return Some(v);
            }
            value = match value.strip_prefix("@dimen/") {
                Some(name) => self.dimens.get(name)?.clone(),
                None => self.theme_attr(theme_attr_name(&value)?)?,
            };
        }
        None
    }

    /// Resolves `@string/...`. Literal text gets the same escape handling
    /// (`\n`, `\'`) Android applies to attribute values.
    pub fn resolve_string(&self, value: &str) -> String {
        if !value.starts_with("@string/") {
            return unescape_string(value);
        }
        let mut value = value.to_string();
        for _ in 0..MAX_INDIRECTION {
            match value
                .strip_prefix("@string/")
                .and_then(|n| self.strings.get(n))
            {
                Some(next) => value = next.clone(),
                None => break,
            }
        }
        value
    }

    /// Looks up an item in a style, following its parents (both explicit
    /// `parent="..."` and the implicit `Parent.Child` naming).
    pub fn style_item(&self, style_ref: &str, item: &str) -> Option<String> {
        let mut name = style_ref
            .trim()
            .trim_start_matches("@style/")
            .trim_start_matches("?attr/")
            .to_string();
        for _ in 0..MAX_INDIRECTION {
            let style = self.styles.get(&name);
            if let Some(style) = style
                && let Some((_, v)) = style.items.iter().find(|(k, _)| k == item)
            {
                return Some(v.clone());
            }
            name = match style.and_then(|s| s.parent.clone()) {
                Some(p) if !p.is_empty() => p,
                _ => name.rsplit_once('.')?.0.to_string(),
            };
        }
        None
    }

    /// Finds a theme attribute (`colorPrimary`, `android:colorAccent`, ...) in
    /// the project's themes. Without the manifest the active theme is unknown,
    /// so styles named `Theme.*` / `AppTheme*` are searched first.
    pub fn theme_attr(&self, attr: &str) -> Option<String> {
        let bare = attr.trim_start_matches("android:");
        let is_theme = |name: &str| name.starts_with("Theme") || name.starts_with("AppTheme");
        let themes = self.styles.keys().filter(|n| is_theme(n));
        let others = self.styles.keys().filter(|n| !is_theme(n));
        themes.chain(others).find_map(|name| {
            self.style_item(name, bare)
                .or_else(|| self.style_item(name, &format!("android:{bare}")))
        })
    }

    pub fn color_names(&self) -> impl Iterator<Item = &str> {
        self.colors.keys().map(String::as_str)
    }

    pub fn dimen_names(&self) -> impl Iterator<Item = &str> {
        self.dimens.keys().map(String::as_str)
    }

    pub fn style_names(&self) -> impl Iterator<Item = &str> {
        self.styles.keys().map(String::as_str)
    }
}

/// `?attr/colorPrimary`, `?android:attr/textColorPrimary` and `?colorPrimary`
/// all name a theme attribute.
fn theme_attr_name(value: &str) -> Option<&str> {
    let rest = value.strip_prefix('?')?;
    Some(
        rest.strip_prefix("attr/")
            .or_else(|| rest.strip_prefix("android:attr/"))
            .unwrap_or(rest),
    )
}

fn unescape_string(s: &str) -> String {
    let s = s
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s);
    s.replace("\\'", "'")
        .replace("\\\"", "\"")
        .replace("\\n", "\n")
        .replace("\\@", "@")
}

/// The framework colors people actually use.
fn android_color(name: &str) -> Option<Color> {
    Some(match name {
        "white" => Color::rgb(255, 255, 255),
        "black" => Color::rgb(0, 0, 0),
        "transparent" => Color::argb(0, 0, 0, 0),
        "darker_gray" => Color::rgb(0xAA, 0xAA, 0xAA),
        "background_dark" => Color::rgb(0, 0, 0),
        "background_light" => Color::rgb(255, 255, 255),
        "primary_text_dark" => Color::rgb(255, 255, 255),
        "primary_text_light" => Color::rgb(0, 0, 0),
        "holo_blue_light" => Color::rgb(0x33, 0xB5, 0xE5),
        "holo_blue_dark" => Color::rgb(0x00, 0x99, 0xCC),
        "holo_blue_bright" => Color::rgb(0x00, 0xDD, 0xFF),
        "holo_green_light" => Color::rgb(0x99, 0xCC, 0x00),
        "holo_green_dark" => Color::rgb(0x66, 0x99, 0x00),
        "holo_red_light" => Color::rgb(0xFF, 0x44, 0x44),
        "holo_red_dark" => Color::rgb(0xCC, 0x00, 0x00),
        "holo_orange_light" => Color::rgb(0xFF, 0xBB, 0x33),
        "holo_orange_dark" => Color::rgb(0xFF, 0x88, 0x00),
        "holo_purple" => Color::rgb(0xAA, 0x66, 0xCC),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Resources {
        let mut r = Resources::default();
        r.add_values_xml(
            r##"<?xml version="1.0" encoding="utf-8"?>
<resources>
    <color name="purple_500">#FF6200EE</color>
    <color name="primary">@color/purple_500</color>
    <dimen name="gap">12dp</dimen>
    <dimen name="big_gap">@dimen/gap</dimen>
    <string name="title">Rock &amp; \'Roll\'</string>
    <style name="Theme.App" parent="Theme.MaterialComponents.DayNight">
        <item name="colorPrimary">@color/primary</item>
    </style>
    <style name="Title">
        <item name="android:textSize">20sp</item>
        <item name="android:textColor">#333</item>
    </style>
    <style name="Title.Big">
        <item name="android:textSize">32sp</item>
    </style>
</resources>"##,
        );
        r
    }

    #[test]
    fn resolves_references() {
        let r = sample();
        assert_eq!(
            r.resolve_color("@color/primary"),
            Color::parse_hex("#FF6200EE")
        );
        assert_eq!(
            r.resolve_color("?attr/colorPrimary"),
            Color::parse_hex("#FF6200EE")
        );
        assert_eq!(
            r.resolve_color("@android:color/white"),
            Some(Color::rgb(255, 255, 255))
        );
        assert_eq!(r.resolve_color("@color/missing"), None);
        assert_eq!(r.resolve_dimension("@dimen/big_gap", 2.0), Some(12.0));
        assert_eq!(r.resolve_string("@string/title"), "Rock & 'Roll'");
        assert_eq!(r.resolve_string("plain"), "plain");
        assert_eq!(r.resolve_string("two\\nlines"), "two\nlines");
    }

    #[test]
    fn style_inheritance() {
        let r = sample();
        assert_eq!(
            r.style_item("@style/Title.Big", "android:textSize")
                .as_deref(),
            Some("32sp")
        );
        assert_eq!(
            r.style_item("@style/Title.Big", "android:textColor")
                .as_deref(),
            Some("#333")
        );
        assert_eq!(r.style_item("@style/Title.Big", "android:gravity"), None);
    }

    #[test]
    fn finds_res_dir_from_layout_path() {
        let dir = std::env::temp_dir().join(format!("layoutsmith-res-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("res/values")).unwrap();
        std::fs::create_dir_all(dir.join("res/layout")).unwrap();
        std::fs::write(
            dir.join("res/values/colors.xml"),
            "<resources><color name=\"accent\">#123456</color></resources>",
        )
        .unwrap();
        let r = Resources::for_layout(&dir.join("res/layout/main.xml"));
        assert_eq!(
            r.resolve_color("@color/accent"),
            Color::parse_hex("#123456")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
