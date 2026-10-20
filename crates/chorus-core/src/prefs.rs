//! The app's look and layout preferences (PLAN P4): theme (D25), text zoom, and how the Overview
//! groups sessions. Stored as one JSON value; anything unreadable falls back to the defaults.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    Glacier,
    Dusk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "kebab-case")]
pub enum GroupBy {
    #[default]
    Status,
    Project,
    None,
}

pub const MIN_ZOOM: f64 = 0.8;
pub const MAX_ZOOM: f64 = 1.6;
pub const ZOOM_STEP: f64 = 0.1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub theme: Theme,
    /// The whole window's zoom, 0.8 to 1.6.
    pub zoom: f64,
    pub group_by: GroupBy,
    /// Whether the session view shows the strip of other sessions.
    pub session_strip: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { theme: Theme::Glacier, zoom: 1.0, group_by: GroupBy::Status, session_strip: true }
    }
}

impl Prefs {
    /// Stored text as preferences. Each field that is missing, unreadable or out of range takes its
    /// default, so one bad value never loses the others.
    pub fn from_stored(text: Option<&str>) -> Self {
        let mut merged = serde_json::to_value(Self::default()).expect("defaults serialise");
        if let (Some(Value::Object(stored)), Value::Object(base)) = (text.and_then(|t| serde_json::from_str(t).ok()), &mut merged) {
            for (key, value) in stored {
                if !base.contains_key(&key) {
                    continue;
                }
                let before = base.insert(key.clone(), value);
                if serde_json::from_value::<Prefs>(Value::Object(base.clone())).is_err() {
                    base.insert(key, before.expect("default present"));
                }
            }
        }
        let mut p: Prefs = serde_json::from_value(merged).unwrap_or_default();
        p.zoom = clean_zoom(p.zoom);
        p
    }
}

/// A zoom in range, rounded to the step.
pub fn clean_zoom(z: f64) -> f64 {
    if !z.is_finite() {
        return 1.0;
    }
    ((z.clamp(MIN_ZOOM, MAX_ZOOM) / ZOOM_STEP).round() * ZOOM_STEP * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_prefs_fall_back_to_defaults() {
        assert_eq!(Prefs::from_stored(None), Prefs::default());
        assert_eq!(Prefs::from_stored(Some("not json")), Prefs::default());
        let p = Prefs::from_stored(Some(r#"{"theme":"dusk","zoom":9}"#));
        assert_eq!((p.theme, p.zoom, p.group_by), (Theme::Dusk, MAX_ZOOM, GroupBy::Status));
        let p = Prefs::from_stored(Some(r#"{"theme":"neon","groupBy":"project","sessionStrip":false}"#));
        assert_eq!((p.theme, p.group_by, p.session_strip), (Theme::Glacier, GroupBy::Project, false), "a bad theme loses only itself");
        assert_eq!(clean_zoom(1.04), 1.0);
        assert_eq!(clean_zoom(1.26), 1.3);
        assert_eq!(clean_zoom(f64::NAN), 1.0);
    }
}
