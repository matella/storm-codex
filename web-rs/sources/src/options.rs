use storm_web_core::route::query_value;

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
}
