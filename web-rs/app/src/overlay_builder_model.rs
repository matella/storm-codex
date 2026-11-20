use storm_web_core::overlay_layout::{
    OverlayLayout, SaveOverlayLayout, WidgetInstance, WidgetPlacement, WidgetSettings, WidgetType,
    GRID_COLS, GRID_ROWS, MAX_WIDGETS,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl From<&WidgetPlacement> for Rect {
    fn from(p: &WidgetPlacement) -> Self {
        Self {
            x: p.grid_x,
            y: p.grid_y,
            w: p.grid_w,
            h: p.grid_h,
        }
    }
}

impl Rect {
    pub fn drag(self, dx: i32, dy: i32, resize: bool) -> Self {
        if resize {
            Self {
                w: self.w.saturating_add(dx).clamp(1, GRID_COLS - self.x),
                h: self.h.saturating_add(dy).clamp(1, GRID_ROWS - self.y),
                ..self
            }
        } else {
            Self {
                x: self.x.saturating_add(dx).clamp(0, GRID_COLS - self.w),
                y: self.y.saturating_add(dy).clamp(0, GRID_ROWS - self.h),
                ..self
            }
        }
    }

    pub fn apply(self, p: &mut WidgetPlacement) {
        (p.grid_x, p.grid_y, p.grid_w, p.grid_h) = (self.x, self.y, self.w, self.h);
    }

    fn overlaps(self, other: Self) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }
}

pub fn snap_delta(px: f64, extent: f64, cells: i32) -> i32 {
    if extent > 0.0 {
        (px * f64::from(cells) / extent).round() as i32
    } else {
        0
    }
}

#[derive(Clone, Default)]
pub struct Draft {
    pub id: Option<String>,
    pub name: String,
    // Back to front. `id` is a draft-local key, not the replace-on-save server widget ID.
    pub widgets: Vec<WidgetInstance>,
}

impl Draft {
    pub fn add(&mut self, id: String, kind: WidgetType, x: i32, y: i32) -> bool {
        if self.widgets.len() >= MAX_WIDGETS {
            return false;
        }
        use WidgetType::*;
        let (w, h, settings) = match kind {
            Ticker => (26, 5, WidgetSettings::Ticker {}),
            Widget => (20, 10, WidgetSettings::Widget(Default::default())),
            Queue => (40, 23, WidgetSettings::Queue {}),
            NowPlaying => (17, 7, WidgetSettings::NowPlaying(Default::default())),
            Recap => (25, 14, WidgetSettings::Recap {}),
            StreamPool => (24, 9, WidgetSettings::StreamPool {}),
            PatchBanner => (24, 6, WidgetSettings::PatchBanner {}),
            RecordFlash => (19, 7, WidgetSettings::RecordFlash {}),
            Nemesis => (18, 7, WidgetSettings::Nemesis {}),
            Goal => (14, 5, WidgetSettings::Goal {}),
        };
        let used: Vec<_> = self
            .widgets
            .iter()
            .map(|i| Rect::from(&i.placement))
            .collect();
        let x = x.clamp(0, GRID_COLS - w);
        let y = y.clamp(0, GRID_ROWS - h);
        let rect = (0..=GRID_ROWS - h)
            .flat_map(|y| (0..=GRID_COLS - w).map(move |x| Rect { x, y, w, h }))
            .filter(|r| used.iter().all(|other| !r.overlaps(*other)))
            .min_by_key(|r| (r.x - x).pow(2) + (r.y - y).pow(2));
        let Some(Rect { x, y, w, h }) = rect else {
            return false;
        };
        self.widgets.push(WidgetInstance {
            id,
            placement: WidgetPlacement {
                grid_x: x,
                grid_y: y,
                grid_w: w,
                grid_h: h,
                z_order: self.widgets.len() as i32,
                settings,
            },
        });
        true
    }

    pub fn normalize_layers(&mut self) {
        for (index, instance) in self.widgets.iter_mut().enumerate() {
            instance.placement.z_order = index as i32;
        }
    }

    pub fn layer(&mut self, id: &str, delta: i32) {
        if let Some(index) = self.widgets.iter().position(|w| w.id == id) {
            let dest = (index as i32 + delta).clamp(0, self.widgets.len() as i32 - 1) as usize;
            self.widgets.swap(index, dest);
            self.normalize_layers();
        }
    }

    pub fn payload(&self) -> SaveOverlayLayout {
        SaveOverlayLayout {
            name: self.name.trim().into(),
            widgets: self.widgets.iter().map(|w| w.placement.clone()).collect(),
        }
    }

    pub fn apply_save(&mut self, saved: &OverlayLayout) {
        self.id = Some(saved.summary.id.clone());
        self.name = saved.summary.name.clone();
        // The wire response is sorted, and we send exactly one instance per distinct z-order.
        for (local, remote) in self.widgets.iter_mut().zip(&saved.widgets) {
            local.placement = remote.placement.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storm_web_core::overlay_layout::OverlayLayoutSummary;

    #[test]
    fn scaled_pointer_delta_snaps_to_cells() {
        assert_eq!(snap_delta(14.0, 800.0, 80), 1);
        assert_eq!(snap_delta(-16.0, 800.0, 80), -2);
        assert_eq!(snap_delta(44.0, 0.0, 80), 0);
    }

    #[test]
    fn move_and_resize_clamp_independently() {
        let rect = Rect {
            x: 10,
            y: 10,
            w: 20,
            h: 10,
        };
        assert_eq!(
            rect.drag(100, -20, false),
            Rect {
                x: 60,
                y: 0,
                ..rect
            }
        );
        assert_eq!(
            rect.drag(-100, 100, true),
            Rect {
                w: 1,
                h: 35,
                ..rect
            }
        );
    }

    #[test]
    fn new_widgets_use_nearest_free_rectangle() {
        let mut draft = Draft::default();
        assert!(draft.add("draft-1".into(), WidgetType::Goal, 0, 0));
        assert!(draft.add("draft-2".into(), WidgetType::Goal, 0, 0));
        let first = Rect::from(&draft.widgets[0].placement);
        let second = Rect::from(&draft.widgets[1].placement);
        assert!(!first.overlaps(second));
        assert_eq!((second.x, second.y), (0, first.h));
    }

    #[test]
    fn save_uses_layer_order_without_sending_client_keys() {
        let mut draft = Draft::default();
        assert!(draft.add("draft-1".into(), WidgetType::Goal, 0, 0));
        assert!(draft.add("draft-2".into(), WidgetType::Ticker, 0, 0));
        draft.layer("draft-1", 1);
        assert_eq!(
            draft
                .widgets
                .iter()
                .map(|w| w.id.as_str())
                .collect::<Vec<_>>(),
            ["draft-2", "draft-1"]
        );
        let payload = serde_json::to_value(draft.payload()).unwrap();
        assert_eq!(payload["widgets"][1]["z_order"], 1);
        assert!(payload["widgets"][1].get("id").is_none());
    }

    #[test]
    fn canonical_save_keeps_editor_and_preview_ids() {
        let mut draft = Draft::default();
        assert!(draft.add("draft-1".into(), WidgetType::Goal, 0, 0));
        let saved = OverlayLayout {
            summary: OverlayLayoutSummary {
                id: "layout-id".into(),
                name: "Canonical name".into(),
                created_at: String::new(),
                updated_at: String::new(),
            },
            widgets: vec![WidgetInstance {
                id: "new-server-id".into(),
                placement: draft.widgets[0].placement.clone(),
            }],
        };
        draft.apply_save(&saved);
        assert_eq!(draft.widgets[0].id, "draft-1");
        assert_eq!(draft.name, "Canonical name");
        assert_eq!(draft.id.as_deref(), Some("layout-id"));
    }
}
