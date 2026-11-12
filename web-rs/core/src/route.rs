//! Which source a `/v2/...` URL shows, and the few query parameters the sources read (`?me=`, `?mini`, `?reveal`,
//! `?skin=`). Pure, so it is tested natively.

/// One page of the overlay bundle (the OBS sources).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    CastOverlay,
    DraftOverlay,
    Ticker,
    NowPlaying,
    Widget,
    Queue,
}

/// `/v2/ticker` -> `Ticker`. The `/v2` prefix and a trailing slash are optional; unknown paths give `None`.
pub fn page_of(pathname: &str) -> Option<Page> {
    let p = pathname.strip_prefix("/v2").unwrap_or(pathname);
    match p.trim_end_matches('/') {
        "/cast/overlay" => Some(Page::CastOverlay),
        "/draft/overlay" => Some(Page::DraftOverlay),
        "/ticker" => Some(Page::Ticker),
        "/now-playing" => Some(Page::NowPlaying),
        "/widget" => Some(Page::Widget),
        "/queue" => Some(Page::Queue),
        _ => None,
    }
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' => match s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                Some(v) => {
                    out.push(v);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `URLSearchParams.get`: the first value of `key`, decoded (`""` when the key has no value).
pub fn query_value(search: &str, key: &str) -> Option<String> {
    search.trim_start_matches('?').split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (decode(k) == key).then(|| decode(v))
    })
}

/// Decoded `(key, value)` pairs of a query string, in order (`URLSearchParams` entries).
pub fn query_pairs(search: &str) -> Vec<(String, String)> {
    search
        .trim_start_matches('?')
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (decode(k), decode(v))
        })
        .collect()
}

/// `URLSearchParams.has`.
pub fn query_has(search: &str, key: &str) -> bool {
    search
        .trim_start_matches('?')
        .split('&')
        .any(|kv| decode(kv.split_once('=').map_or(kv, |(k, _)| k)) == key)
}

/// Position of the content inside the 1920x1080 frame (`OverlayFrame`): the corner with a constant `pad`, then
/// explicit `top`/`right`/`bottom`/`left` offsets (px) win over the corner. Properties keep the insertion order
/// of the React component, so the style attribute is identical.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Offsets {
    pub top: Option<f64>,
    pub right: Option<f64>,
    pub bottom: Option<f64>,
    pub left: Option<f64>,
}

pub fn frame_position(anchor: Anchor, pad: f64, o: Offsets) -> String {
    let mut pos: Vec<(&str, f64)> = vec![];
    // insert or overwrite in place (a JS object keeps the original key position)
    fn set<'a>(pos: &mut Vec<(&'a str, f64)>, k: &'a str, v: f64) {
        match pos.iter_mut().find(|(x, _)| *x == k) {
            Some(e) => e.1 = v,
            None => pos.push((k, v)),
        }
    }
    fn del(pos: &mut Vec<(&str, f64)>, k: &str) {
        pos.retain(|(x, _)| *x != k);
    }
    if matches!(anchor, Anchor::TopLeft | Anchor::TopRight) {
        set(&mut pos, "top", pad);
    } else {
        set(&mut pos, "bottom", pad);
    }
    if matches!(anchor, Anchor::TopLeft | Anchor::BottomLeft) {
        set(&mut pos, "left", pad);
    } else {
        set(&mut pos, "right", pad);
    }
    if let Some(v) = o.top {
        set(&mut pos, "top", v);
        del(&mut pos, "bottom");
    }
    if let Some(v) = o.bottom {
        set(&mut pos, "bottom", v);
        del(&mut pos, "top");
    }
    if let Some(v) = o.left {
        set(&mut pos, "left", v);
        del(&mut pos, "right");
    }
    if let Some(v) = o.right {
        set(&mut pos, "right", v);
        del(&mut pos, "left");
    }
    pos.iter()
        .map(|(k, v)| format!("{k}:{v}px"))
        .collect::<Vec<_>>()
        .join(";")
}

/// Prefix of the app while it is served next to React (`/v2/...`). After the switch the server serves the app at `/`
/// and the base is empty; the same build handles both, decided once at startup from the location (`init_base`).
pub const V2_BASE: &str = "/v2";

static ROOT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Base of a location pathname: `/v2` for `/v2` and `/v2/...`, empty otherwise.
pub fn base_of(pathname: &str) -> &'static str {
    match pathname.strip_prefix(V2_BASE) {
        Some("") => V2_BASE,
        Some(rest) if rest.starts_with('/') => V2_BASE,
        _ => "",
    }
}

/// Decides the base from the page that was loaded (call once, before mounting).
pub fn init_base(pathname: &str) {
    ROOT.store(
        base_of(pathname).is_empty(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Prefix of the app under the server: `/v2` or empty (served at the root).
pub fn app_base() -> &'static str {
    if ROOT.load(std::sync::atomic::Ordering::Relaxed) {
        ""
    } else {
        V2_BASE
    }
}

/// Real `href` of an app path (`/matches` -> `/v2/matches`, or unchanged at the root): the router only takes over
/// clicks on links inside its base, and middle-click / copy-link need the full URL.
pub fn app_href(to: &str) -> String {
    format!("{}{to}", app_base())
}

/// App path of a location pathname (`/v2/matches` -> `/matches`, `/v2` -> `/`).
pub fn app_path(pathname: &str) -> &str {
    app_path_in(app_base(), pathname)
}

/// [`app_path`] for an explicit base.
pub fn app_path_in<'a>(base: &str, pathname: &'a str) -> &'a str {
    if base.is_empty() {
        return pathname;
    }
    match pathname.strip_prefix(base) {
        Some("") => "/",
        Some(rest) if rest.starts_with('/') => rest,
        _ => pathname,
    }
}

/// `encodeURIComponent`: everything but `A-Za-z0-9 - _ . ! ~ * ' ( )` is percent-encoded (UTF-8).
pub fn uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Top-bar tabs of the app: (path, label). Same order as `Layout.tsx`.
pub const TABS: [(&str, &str); 13] = [
    ("/", "Session"),
    ("/live", "Live"),
    ("/matches", "Matches"),
    ("/players", "Players"),
    ("/heroes", "Heroes"),
    ("/maps", "Maps"),
    ("/synergies", "Synergies"),
    ("/patches", "Patch Notes"),
    ("/trends", "Trends"),
    ("/leagues", "Leagues"),
    ("/cast", "Cast"),
    ("/scouting", "Scouting"),
    ("/admin", "Admin"),
];

/// `NavLink` active rule: the home tab only on `/`, every other tab on its path and below (`/matches`,
/// `/matches/...`, not `/matchesx`). `pathname` is the app path, without the `/v2` prefix.
pub fn tab_active(pathname: &str, to: &str) -> bool {
    let p = pathname.trim_end_matches('/');
    if to == "/" {
        return p.is_empty();
    }
    p == to || p.strip_prefix(to).is_some_and(|r| r.starts_with('/'))
}

/// Old pages absorbed by the cast control room (`legacyRoutes.ts`): path -> destination.
pub const LEGACY_REDIRECTS: [(&str, &str); 2] = [
    ("/draft", "/cast?scene=draft"),
    ("/draft/replay", "/cast?scene=draft"),
];

/// Where an old path redirects to, if it is one.
pub fn legacy_redirect(pathname: &str) -> Option<&'static str> {
    let p = pathname.trim_end_matches('/');
    LEGACY_REDIRECTS
        .iter()
        .find(|(f, _)| *f == p)
        .map(|(_, t)| *t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_are_found_with_or_without_the_prefix() {
        assert_eq!(page_of("/v2/ticker"), Some(Page::Ticker));
        assert_eq!(page_of("/v2/draft/overlay/"), Some(Page::DraftOverlay));
        assert_eq!(page_of("/cast/overlay"), Some(Page::CastOverlay));
        assert_eq!(page_of("/v2/now-playing"), Some(Page::NowPlaying));
        assert_eq!(page_of("/v2/queue"), Some(Page::Queue));
        assert_eq!(page_of("/v2/widget"), Some(Page::Widget));
        assert_eq!(page_of("/v2/"), None);
        assert_eq!(page_of("/v2/matches"), None);
    }

    #[test]
    fn query_parameters_read_like_urlsearchparams() {
        assert_eq!(
            query_value("?me=matella&x=1", "me").as_deref(),
            Some("matella")
        );
        assert_eq!(
            query_value("?me=Jean%20Luc+X", "me").as_deref(),
            Some("Jean Luc X")
        );
        assert_eq!(query_value("?mini", "mini").as_deref(), Some(""));
        assert_eq!(query_value("?a=1", "me"), None);
        assert_eq!(query_value("", "me"), None);
        assert!(query_has("?mini&reveal", "reveal"));
        assert!(query_has("?reveal=1", "reveal"));
        assert!(!query_has("?minimal", "mini"));
        assert!(!query_has("", "mini"));
        assert_eq!(query_value("?me=100%", "me").as_deref(), Some("100%"));
    }

    #[test]
    fn frame_position_follows_the_react_component() {
        let none = Offsets::default();
        assert_eq!(
            frame_position(Anchor::TopLeft, 28.0, none),
            "top:28px;left:28px"
        );
        assert_eq!(
            frame_position(Anchor::TopRight, 36.0, none),
            "top:36px;right:36px"
        );
        assert_eq!(
            frame_position(Anchor::BottomLeft, 10.0, none),
            "bottom:10px;left:10px"
        );
        // explicit top replaces the pad in place
        let top = Offsets {
            top: Some(250.0),
            ..none
        };
        assert_eq!(
            frame_position(Anchor::TopRight, 28.0, top),
            "top:250px;right:28px"
        );
        // explicit top on a bottom anchor drops the bottom and appends
        assert_eq!(
            frame_position(Anchor::BottomRight, 28.0, top),
            "right:28px;top:250px"
        );
        // explicit left drops right
        let left = Offsets {
            left: Some(5.5),
            ..none
        };
        assert_eq!(
            frame_position(Anchor::TopRight, 28.0, left),
            "top:28px;left:5.5px"
        );
    }

    #[test]
    fn tabs_are_active_on_their_path_and_below() {
        assert!(tab_active("/", "/"));
        assert!(!tab_active("/matches", "/"));
        assert!(tab_active("/matches", "/matches"));
        assert!(tab_active("/matches/", "/matches"));
        assert!(!tab_active("/match/12", "/matches"));
        assert!(tab_active("/scouting/7", "/scouting"));
        assert!(!tab_active("/scoutingx", "/scouting"));
    }

    #[test]
    fn legacy_paths_redirect_to_the_cast_room() {
        assert_eq!(legacy_redirect("/draft"), Some("/cast?scene=draft"));
        assert_eq!(legacy_redirect("/draft/replay/"), Some("/cast?scene=draft"));
        assert_eq!(legacy_redirect("/cast"), None);
    }

    #[test]
    fn links_and_locations_convert_with_the_base() {
        assert_eq!(app_href("/matches"), "/v2/matches");
        assert_eq!(app_href("/cast?scene=draft"), "/v2/cast?scene=draft");
        assert_eq!(app_path("/v2/matches"), "/matches");
        assert_eq!(app_path("/v2"), "/");
        assert_eq!(app_path("/v2/"), "/");
        assert_eq!(app_path("/v2x/a"), "/v2x/a");
        // served at the root: nothing to strip
        assert_eq!(app_path_in("", "/matches"), "/matches");
        assert_eq!(app_path_in("", "/"), "/");
        assert_eq!(base_of("/v2/cast"), "/v2");
        assert_eq!(base_of("/v2"), "/v2");
        assert_eq!(base_of("/v2x"), "");
        assert_eq!(base_of("/cast"), "");
    }

    #[test]
    fn uri_components_are_encoded_like_the_browser_does() {
        assert_eq!(uri_component("Li-Ming"), "Li-Ming");
        assert_eq!(uri_component("E.T.C."), "E.T.C.");
        assert_eq!(uri_component("The Lost Vikings"), "The%20Lost%20Vikings");
        assert_eq!(uri_component("Blackheart's Bay"), "Blackheart's%20Bay");
        assert_eq!(uri_component("Lúcio#1/2"), "L%C3%BAcio%231%2F2");
    }
}
