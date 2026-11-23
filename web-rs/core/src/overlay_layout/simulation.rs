//! Builder preview protocol; these flags are not part of the saved layout.

use super::WidgetType;

pub const MESSAGE: &str = "storm-overlay-simulate";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    PatchBanner,
    Nemesis,
    RecordFlash,
}

impl Kind {
    pub const ALL: [Self; 3] = [Self::PatchBanner, Self::Nemesis, Self::RecordFlash];

    pub fn from_widget(widget: WidgetType) -> Option<Self> {
        match widget {
            WidgetType::PatchBanner => Some(Self::PatchBanner),
            WidgetType::Nemesis => Some(Self::Nemesis),
            WidgetType::RecordFlash => Some(Self::RecordFlash),
            _ => None,
        }
    }

    pub fn wire(self) -> &'static str {
        match self {
            Self::PatchBanner => "patch-banner",
            Self::Nemesis => "nemesis",
            Self::RecordFlash => "record-flash",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Simulations([bool; 3]);

impl Simulations {
    pub fn active(self, kind: Kind) -> bool {
        self.0[kind as usize]
    }

    pub fn set(&mut self, kind: Kind, active: bool) {
        self.0[kind as usize] = active;
    }
}

/// Decode fields after the browser adapter has checked the origin, peer and field types.
pub fn parse(kind: &str, active: bool) -> Option<(Kind, bool)> {
    Kind::ALL
        .into_iter()
        .find(|k| k.wire() == kind)
        .map(|kind| (kind, active))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_messages_toggle_only_the_named_kind() {
        for (wire, kind) in [
            ("patch-banner", Kind::PatchBanner),
            ("nemesis", Kind::Nemesis),
            ("record-flash", Kind::RecordFlash),
        ] {
            let mut state = Simulations::default();
            for active in [true, false] {
                let (decoded, value) = parse(wire, active).unwrap();
                state.set(decoded, value);
                assert_eq!((decoded, value), (kind, active));
                for candidate in Kind::ALL {
                    assert_eq!(state.active(candidate), candidate == kind && active);
                }
            }
        }
    }

    #[test]
    fn unknown_and_unsupported_kinds_are_ignored() {
        for wire in ["unknown", "", "ticker", "PatchBanner"] {
            for active in [true, false] {
                assert_eq!(parse(wire, active), None);
            }
        }
    }
}
