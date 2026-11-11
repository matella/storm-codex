//! What the app does with each event of the `/ws` channel (`useLiveUpdates` of api.ts): which data must be re-read,
//! and the toast of the top bar. Pure, so it is tested natively.

/// Effects of one event. Each `*_changed` flag makes the pages that show that data read it again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    /// `match.parsed`: match lists and hero statistics.
    pub matches_changed: bool,
    /// `patch.new`: patch lists.
    pub patches_changed: bool,
    /// `draft.updated`.
    pub draft_changed: bool,
    /// `cast.updated`: the cast state and the lineups.
    pub cast_changed: bool,
    /// `live.updated`: stateless signal, the filtered view is read again by `GET /api/live`.
    pub live_changed: bool,
    /// The top bar shows "online" (the callback of the React hook fires for these events only).
    pub online: bool,
    /// A new patch: dot on the Patch Notes tab.
    pub new_patch: bool,
    /// Toast text (disappears after 6 s).
    pub flash: Option<String>,
}

/// Effects of an event of this `kind` (`map` of a parsed match, `name` of a new patch).
pub fn effects(kind: &str, map: Option<&str>, name: Option<&str>) -> Effects {
    match kind {
        "match.parsed" => Effects {
            matches_changed: true,
            online: true,
            flash: Some(format!("● new replay — {} added", map.unwrap_or("match"))),
            ..Effects::default()
        },
        "patch.new" => Effects {
            patches_changed: true,
            online: true,
            new_patch: true,
            flash: Some(format!("🆕 new patch — {}", name.unwrap_or("patch notes"))),
            ..Effects::default()
        },
        // React also flashed "new replay" here, by mistake: a draft change is not a replay
        "draft.updated" => Effects {
            draft_changed: true,
            online: true,
            ..Effects::default()
        },
        "cast.updated" => Effects {
            cast_changed: true,
            ..Effects::default()
        },
        "live.updated" => Effects {
            live_changed: true,
            ..Effects::default()
        },
        _ => Effects::default(),
    }
}

/// After a reconnection the events missed in between are lost: the cast and draft state are read again
/// (`reopenInvalidations`). Nothing after the first connection.
pub fn reopen_effects(had_disconnect: bool) -> Effects {
    Effects {
        cast_changed: had_disconnect,
        draft_changed: had_disconnect,
        ..Effects::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parsed_match_refreshes_lists_and_flashes_the_map() {
        let e = effects("match.parsed", Some("Cursed Hollow"), None);
        assert!(e.matches_changed && e.online);
        assert_eq!(
            e.flash.as_deref(),
            Some("● new replay — Cursed Hollow added")
        );
        assert_eq!(
            effects("match.parsed", None, None).flash.as_deref(),
            Some("● new replay — match added")
        );
    }

    #[test]
    fn a_new_patch_marks_the_tab_and_flashes_its_name() {
        let e = effects("patch.new", None, Some("2026.11 Patch"));
        assert!(e.patches_changed && e.new_patch);
        assert_eq!(e.flash.as_deref(), Some("🆕 new patch — 2026.11 Patch"));
        assert_eq!(
            effects("patch.new", None, None).flash.as_deref(),
            Some("🆕 new patch — patch notes")
        );
    }

    #[test]
    fn cast_and_live_signals_are_silent() {
        assert_eq!(
            effects("cast.updated", None, None),
            Effects {
                cast_changed: true,
                ..Effects::default()
            }
        );
        let l = effects("live.updated", None, None);
        assert!(l.live_changed && l.flash.is_none() && !l.online);
        assert_eq!(effects("poll", None, None), Effects::default());
        assert!(effects("draft.updated", None, None).flash.is_none());
    }

    #[test]
    fn only_a_reconnection_rereads_cast_and_draft() {
        assert_eq!(reopen_effects(false), Effects::default());
        let e = reopen_effects(true);
        assert!(e.cast_changed && e.draft_changed && e.flash.is_none());
    }
}
