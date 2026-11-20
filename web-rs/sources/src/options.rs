use storm_web_core::route::{query_has, query_value};

/// Explicit last-game perspective. `None` uses the configured operator names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WidgetOptions {
    pub me: Option<String>,
}

impl WidgetOptions {
    pub fn from_search(search: &str) -> Self {
        Self {
            me: query_value(search, "me"),
        }
    }
}

/// The three presentations of the same music feed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NowPlayingMode {
    #[default]
    Full,
    Mini,
    Reveal,
}

impl NowPlayingMode {
    pub fn from_search(search: &str) -> Self {
        if query_has(search, "reveal") {
            Self::Reveal
        } else if query_has(search, "mini") {
            Self::Mini
        } else {
            Self::Full
        }
    }

    pub fn poll_ms(self) -> u64 {
        // A reveal needs to react quickly to a new track.
        if self == Self::Reveal {
            2_000
        } else {
            5_000
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_query_uses_the_first_decoded_name() {
        assert_eq!(
            WidgetOptions::from_search("?me=A%2BB+Player&me=Other")
                .me
                .as_deref(),
            Some("A+B Player")
        );
    }

    #[test]
    fn widget_query_leaves_missing_name_to_the_operator_fallback() {
        assert_eq!(WidgetOptions::from_search("?mini").me, None);
        assert_eq!(WidgetOptions::from_search("?me=").me.as_deref(), Some(""));
    }

    #[test]
    fn music_query_defaults_to_the_full_card() {
        assert_eq!(
            NowPlayingMode::from_search("?unknown=mini"),
            NowPlayingMode::Full
        );
    }

    #[test]
    fn mini_flag_uses_presence_not_value() {
        assert_eq!(NowPlayingMode::from_search("?mini=0"), NowPlayingMode::Mini);
    }

    #[test]
    fn reveal_wins_over_mini_at_the_faster_cadence() {
        assert_eq!(
            NowPlayingMode::from_search("?mini&%72eveal=false"),
            NowPlayingMode::Reveal
        );
        assert_eq!(NowPlayingMode::Reveal.poll_ms(), 2_000);
        assert_eq!(NowPlayingMode::Mini.poll_ms(), 5_000);
        assert_eq!(NowPlayingMode::Full.poll_ms(), 5_000);
    }
}
