/// Largest uniform scale that fits the measured CSS box in the placement, if nonempty.
pub fn scale_to_fit((width, height): (f64, f64), (w, h): (f64, f64)) -> Option<f64> {
    (w > 0.0 && h > 0.0).then(|| (width / w).min(height / h))
}

#[cfg(test)]
mod tests {
    use super::scale_to_fit;

    #[test]
    fn exact_fit_keeps_native_size() {
        assert_eq!(scale_to_fit((240.0, 120.0), (240.0, 120.0)), Some(1.0));
    }

    #[test]
    fn smaller_content_scales_up() {
        assert_eq!(scale_to_fit((240.0, 120.0), (120.0, 60.0)), Some(2.0));
    }

    #[test]
    fn larger_content_scales_down() {
        assert_eq!(scale_to_fit((240.0, 120.0), (480.0, 240.0)), Some(0.5));
    }

    #[test]
    fn mismatched_aspect_ratio_uses_the_limiting_axis() {
        assert_eq!(scale_to_fit((240.0, 120.0), (120.0, 240.0)), Some(0.5));
        assert_eq!(scale_to_fit((240.0, 120.0), (480.0, 60.0)), Some(0.5));
    }

    #[test]
    fn empty_content_has_no_scale() {
        assert_eq!(scale_to_fit((240.0, 120.0), (240.0, 0.0)), None);
        assert_eq!(scale_to_fit((240.0, 120.0), (0.0, 120.0)), None);
    }
}
