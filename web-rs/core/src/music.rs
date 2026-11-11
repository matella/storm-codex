//! Music sources (`/now-playing`, `/queue`): parse the Orpheus proxy answer, track identity, progress, and the
//! "reveal" state machine. Port of `parseTrack`/`trackKey` (api.ts), `revealState.ts` and `useTrackReveal.ts`.

use serde_json::Value;

/// Display time of the big reveal card before it folds into the badge (spec: fixed at 2.6 s).
pub const HOLD_MS: u32 = 2600;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub playing: bool,
    /// Stable id of the track (`id` or Spotify `uri`); absent on the old engine.
    pub id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub art: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<f64>,
    pub progress_ms: Option<f64>,
}

fn str_of(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_string)
}

fn num_of(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}

/// `parseTrack`. The proxy wraps the Orpheus answer in `current`; two shapes exist: Spotify live
/// (`{isPlaying, track:{name, artists:[{name}], album:{images:[{url}]}}}`) and the old engine
/// (`{current:{name, artist, albumArtUrl}, isPlaying}`). Both are read, with the field variants.
pub fn parse_track(np: &Value) -> Track {
    let null = Value::Null;
    let o = np.get("current").unwrap_or(&null);
    // `o.track ?? o.current ?? o`: first non-null
    let t = [o.get("track"), o.get("current")]
        .into_iter()
        .flatten()
        .find(|v| !v.is_null())
        .unwrap_or(o);
    let title = str_of(t.get("name"))
        .or_else(|| str_of(t.get("title")))
        .or_else(|| str_of(t.get("track")));
    let id = str_of(t.get("id")).or_else(|| str_of(t.get("uri")));
    // artist: Spotify array of {name} joined, else a plain string
    let artist = match t.get("artists") {
        Some(Value::Array(a)) => {
            let names: Vec<String> = a
                .iter()
                .filter_map(|x| str_of(x.get("name")))
                .filter(|n| !n.is_empty())
                .collect();
            (!names.is_empty()).then(|| names.join(", "))
        }
        other => str_of(t.get("artist"))
            .or_else(|| str_of(other))
            .or_else(|| str_of(t.get("author"))),
    };
    let album_obj = t.get("album");
    let art = album_obj
        .and_then(|a| a.get("images"))
        .and_then(Value::as_array)
        .and_then(|i| i.first())
        .and_then(|i| str_of(i.get("url")))
        .or_else(|| str_of(t.get("albumArtUrl")))
        .or_else(|| str_of(t.get("albumArt")))
        .or_else(|| str_of(t.get("image")));
    let album = album_obj
        .and_then(|a| str_of(a.get("name")))
        .or_else(|| str_of(album_obj));
    let duration_ms = num_of(t.get("durationMs")).or_else(|| num_of(t.get("duration")));
    let progress_ms = num_of(o.get("progressMs"));
    let is_playing = o.get("isPlaying") != Some(&Value::Bool(false)); // absent: assume playing
    let authenticated = np
        .get("authenticated")
        .is_some_and(|a| !matches!(a, Value::Null | Value::Bool(false)));
    Track {
        playing: authenticated && title.as_deref().is_some_and(|t| !t.is_empty()) && is_playing,
        id,
        title,
        artist,
        art,
        album,
        duration_ms,
        progress_ms,
    }
}

/// `trackKey`: identity of a track to spot a change. `id` when the source gives one, else `title|artist`;
/// `None` = nothing usable, so nothing to announce.
pub fn track_key(t: &Track) -> Option<String> {
    if let Some(id) = t.id.as_deref().filter(|i| !i.is_empty()) {
        return Some(id.to_string());
    }
    t.title
        .as_deref()
        .filter(|x| !x.is_empty())
        .map(|title| format!("{title}|{}", t.artist.as_deref().unwrap_or("")))
}

/// `m:ss` of a millisecond count (never negative).
pub fn fmt_ms(ms: f64) -> String {
    let s = (ms / 1000.0).floor().max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Progress bar width, percent of the duration (0 without a duration).
pub fn progress_pct(progress_ms: f64, duration_ms: Option<f64>) -> f64 {
    match duration_ms {
        Some(d) if d > 0.0 => (progress_ms / d * 100.0).min(100.0),
        _ => 0.0,
    }
}

/// `hidden` = nothing on screen; `big` = large announcement card; `mini` = persistent compact badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealPhase {
    Hidden,
    Big,
    Mini,
}

impl RevealPhase {
    pub fn class(self) -> &'static str {
        match self {
            RevealPhase::Hidden => "is-hidden",
            RevealPhase::Big => "is-big",
            RevealPhase::Mini => "is-mini",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevealState {
    pub phase: RevealPhase,
    /// Key of the track on the card; `None` when hidden.
    pub key: Option<String>,
    /// Bumped each time the hold must be (re)armed.
    pub hold_token: u32,
}

impl Default for RevealState {
    fn default() -> Self {
        RevealState {
            phase: RevealPhase::Hidden,
            key: None,
            hold_token: 0,
        }
    }
}

/// Transition on a poll update. Every start of playback announces (new track, resume after a pause, first
/// mount with music playing); a track change during the big card re-targets it in place (stays `Big`, hold
/// restarts) so the card does not yo-yo when skipping. Returns `prev` unchanged when nothing changes.
pub fn next_reveal_state(prev: &RevealState, playing: bool, key: Option<&str>) -> RevealState {
    let Some(key) = key.filter(|_| playing) else {
        return if prev.phase == RevealPhase::Hidden {
            prev.clone()
        } else {
            RevealState {
                phase: RevealPhase::Hidden,
                key: None,
                hold_token: prev.hold_token,
            }
        };
    };
    if prev.phase == RevealPhase::Hidden || prev.key.as_deref() != Some(key) {
        return RevealState {
            phase: RevealPhase::Big,
            key: Some(key.to_string()),
            hold_token: prev.hold_token + 1,
        };
    }
    prev.clone()
}

/// Transition when the hold expires: the big card folds into the badge.
pub fn hold_expired(prev: &RevealState) -> RevealState {
    if prev.phase == RevealPhase::Big {
        RevealState {
            phase: RevealPhase::Mini,
            ..prev.clone()
        }
    } else {
        prev.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spotify_shape_is_parsed() {
        let t = parse_track(&json!({
            "authenticated": true,
            "current": {
                "isPlaying": true, "progressMs": 1500,
                "track": { "id": "abc", "name": "Song", "artists": [{"name":"A"},{"name":"B"}],
                           "album": {"name":"Alb","images":[{"url":"http://x/a.jpg"}]}, "durationMs": 200000 }
            }
        }));
        assert!(t.playing);
        assert_eq!(t.title.as_deref(), Some("Song"));
        assert_eq!(t.artist.as_deref(), Some("A, B"));
        assert_eq!(t.art.as_deref(), Some("http://x/a.jpg"));
        assert_eq!(t.album.as_deref(), Some("Alb"));
        assert_eq!(t.duration_ms, Some(200000.0));
        assert_eq!(t.progress_ms, Some(1500.0));
        assert_eq!(track_key(&t).as_deref(), Some("abc"));
    }

    #[test]
    fn old_engine_shape_and_states_that_hide_the_card() {
        let t = parse_track(&json!({
            "authenticated": true,
            "current": { "isPlaying": true, "current": { "name": "Old", "artist": "Art", "albumArtUrl": "u" } }
        }));
        assert!(t.playing);
        assert_eq!(
            (t.artist.as_deref(), t.art.as_deref()),
            (Some("Art"), Some("u"))
        );
        assert_eq!(track_key(&t).as_deref(), Some("Old|Art"));
        // paused, signed out, nothing
        assert!(!parse_track(&json!({"authenticated": true, "current": {"isPlaying": false, "track": {"name": "x"}}})).playing);
        assert!(
            !parse_track(&json!({"authenticated": false, "current": {"track": {"name": "x"}}}))
                .playing
        );
        assert!(!parse_track(&json!({"authenticated": true, "current": {}})).playing);
        assert!(!parse_track(&Value::Null).playing);
        assert_eq!(track_key(&Track::default()), None);
        // empty artists array -> no artist
        let e = parse_track(
            &json!({"authenticated": true, "current": {"track": {"name": "x", "artists": []}}}),
        );
        assert_eq!(e.artist, None);
    }

    #[test]
    fn clock_and_progress() {
        assert_eq!(fmt_ms(65_999.0), "1:05");
        assert_eq!(fmt_ms(-5.0), "0:00");
        assert_eq!(progress_pct(50.0, Some(200.0)), 25.0);
        assert_eq!(progress_pct(500.0, Some(200.0)), 100.0);
        assert_eq!(progress_pct(5.0, None), 0.0);
    }

    #[test]
    fn reveal_announces_every_start_and_retargets_in_place() {
        let s0 = RevealState::default();
        // nothing playing: unchanged
        assert_eq!(next_reveal_state(&s0, false, Some("a")), s0);
        // first mount with music: announce
        let s1 = next_reveal_state(&s0, true, Some("a"));
        assert_eq!((s1.phase, s1.hold_token), (RevealPhase::Big, 1));
        // same poll again: same state (timer not re-armed)
        assert_eq!(next_reveal_state(&s1, true, Some("a")), s1);
        // skip during the big card: stays big, hold restarts
        let s2 = next_reveal_state(&s1, true, Some("b"));
        assert_eq!(
            (s2.phase, s2.hold_token, s2.key.as_deref()),
            (RevealPhase::Big, 2, Some("b"))
        );
        // hold expires: badge; a poll of the same track leaves it
        let s3 = hold_expired(&s2);
        assert_eq!(s3.phase, RevealPhase::Mini);
        assert_eq!(next_reveal_state(&s3, true, Some("b")), s3);
        assert_eq!(hold_expired(&s3), s3);
        // a new track announces again
        assert_eq!(
            next_reveal_state(&s3, true, Some("c")).phase,
            RevealPhase::Big
        );
        // pause hides, resume announces the same track again
        let s4 = next_reveal_state(&s3, false, Some("b"));
        assert_eq!((s4.phase, s4.key.clone()), (RevealPhase::Hidden, None));
        assert_eq!(
            next_reveal_state(&s4, true, Some("b")).phase,
            RevealPhase::Big
        );
        // playing without a usable key hides
        assert_eq!(
            next_reveal_state(&s3, true, None).phase,
            RevealPhase::Hidden
        );
    }
}
