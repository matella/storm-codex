//! Allowlists of the HTML sanitiser (`SafeHtml` of the app): which tags, attributes, links and styles of external
//! HTML (patch notes) may reach the page. The DOM walk that applies them lives in the app crate.
//! Replaces DOMPurify of the React front-end; `ammonia` was measured at about 300 kB gzip, too heavy for a page.

/// Tags kept as they are (the usual formatting, lists, tables, links and images of patch notes).
const TAGS: &[&str] = &[
    "a",
    "abbr",
    "b",
    "blockquote",
    "br",
    "caption",
    "code",
    "dd",
    "del",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "mark",
    "ol",
    "p",
    "pre",
    "q",
    "s",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "u",
    "ul",
];

/// Tags removed with everything inside (active content). Any other unknown tag is unwrapped: its content stays.
const DROPPED: &[&str] = &[
    "script", "style", "iframe", "frame", "frameset", "object", "embed", "applet", "noscript",
    "template", "svg", "math", "link", "meta", "base", "form", "input", "button", "select",
    "textarea", "audio", "video", "canvas", "title", "head",
];

pub fn tag_allowed(tag: &str) -> bool {
    TAGS.contains(&tag.to_ascii_lowercase().as_str())
}

pub fn tag_dropped(tag: &str) -> bool {
    DROPPED.contains(&tag.to_ascii_lowercase().as_str())
}

/// Attributes kept on any allowed tag; `id` only when the caller keeps anchors.
pub fn attr_allowed(tag: &str, name: &str, keep_ids: bool) -> bool {
    let (tag, name) = (tag.to_ascii_lowercase(), name.to_ascii_lowercase());
    match name.as_str() {
        "class" | "style" | "title" | "colspan" | "rowspan" | "width" | "height" | "align"
        | "valign" | "dir" | "lang" => true,
        "id" => keep_ids,
        "href" => tag == "a",
        "src" | "alt" => tag == "img",
        _ => false,
    }
}

/// A link target that cannot run code: `http(s)`, `mailto`, `tel`, a fragment or a relative path. Control
/// characters and whitespace inside the scheme are stripped before the check (`java\tscript:` is still a script).
/// `data:` images are allowed for `src` only.
pub fn url_safe(url: &str, is_image_src: bool) -> bool {
    let cleaned: String = url
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let scheme_end = cleaned.find([':', '/', '?', '#']);
    match scheme_end {
        Some(i) if cleaned.as_bytes()[i] == b':' => {
            let scheme = &cleaned[..i];
            matches!(scheme, "http" | "https" | "mailto" | "tel")
                || (is_image_src
                    && scheme == "data"
                    && cleaned[i + 1..].starts_with("image/")
                    && !cleaned[i + 1..].starts_with("image/svg"))
        }
        _ => true,
    }
}

/// An external link shown as a button (`officialLink`): an absolute `http(s)` URL that passes `url_safe`.
pub fn is_web_link(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    (u.starts_with("https://") || u.starts_with("http://")) && url_safe(url, false)
}

/// A style that cannot load anything or run code (no `url(`, `expression`, `@import`, `javascript:`, backslash
/// escapes or comments, which hide the others).
pub fn style_safe(style: &str) -> bool {
    let s = style.to_ascii_lowercase();
    ![
        "url(",
        "expression",
        "@import",
        "javascript:",
        "behavior",
        "-moz-binding",
        "\\",
        "/*",
    ]
    .iter()
    .any(|bad| s.contains(bad))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_patch_tags_pass_and_active_content_does_not() {
        for t in [
            "a",
            "b",
            "blockquote",
            "div",
            "h3",
            "li",
            "p",
            "s",
            "span",
            "strong",
            "ul",
            "UL",
        ] {
            assert!(tag_allowed(t), "{t}");
        }
        for t in ["script", "iframe", "svg", "style", "SCRIPT"] {
            assert!(!tag_allowed(t) && tag_dropped(t), "{t}");
        }
        assert!(!tag_allowed("font") && !tag_dropped("font"));
    }

    #[test]
    fn only_inert_attributes_are_kept() {
        assert!(attr_allowed("a", "href", false));
        assert!(!attr_allowed("p", "href", false));
        assert!(attr_allowed("img", "src", false));
        assert!(attr_allowed("span", "style", false));
        assert!(attr_allowed("h3", "ID", true));
        assert!(!attr_allowed("h3", "id", false));
        assert!(!attr_allowed("a", "onclick", true));
        assert!(!attr_allowed("a", "target", true));
    }

    #[test]
    fn links_cannot_run_code() {
        for u in [
            "https://a.b/c",
            "http://x",
            "/patch/x#a",
            "#top",
            "page?x=1:2",
            "mailto:a@b.c",
        ] {
            assert!(url_safe(u, false), "{u}");
        }
        for u in [
            "javascript:alert(1)",
            " JaVa\tScRiPt:x",
            "vbscript:x",
            "data:text/html,x",
            "\u{1}javascript:x",
        ] {
            assert!(!url_safe(u, false), "{u}");
        }
        assert!(url_safe("data:image/png;base64,AAAA", true));
        assert!(!url_safe("data:image/png;base64,AAAA", false));
        assert!(!url_safe("data:image/svg+xml,<svg/>", true));
    }

    #[test]
    fn the_official_link_must_be_an_http_url() {
        assert!(is_web_link("https://heroesofthestorm.blizzard.com/x"));
        assert!(is_web_link("HTTP://a.b"));
        for u in [
            "javascript:alert(1)",
            "/relative",
            "",
            "mailto:a@b.c",
            "data:text/html,x",
            " java\tscript:x",
        ] {
            assert!(!is_web_link(u), "{u}");
        }
    }

    #[test]
    fn styles_cannot_load_or_run_anything() {
        assert!(style_safe("color: rgba(153, 0, 255, 1)"));
        assert!(!style_safe("background: url(http://x)"));
        assert!(!style_safe("width: expression(alert(1))"));
        assert!(!style_safe("color: \\72 ed"));
    }
}
