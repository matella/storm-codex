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
}
