//! What the app sends: the ad settings, and where the page's slot is.

use serde::{Deserialize, Serialize};

/// What every request carries. The app decides both.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Treat every request as for a child.
    pub child_directed: bool,
    /// The highest content rating an ad may have: `"G"`, `"PG"`, `"T"` or `"MA"`.
    pub max_rating: String,
}

fn ad() -> String {
    "Ad".to_string()
}

/// The page's empty box that the native ad sits in, in CSS pixels.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub unit_id: String,
    /// The slot's place in the scroll box's content.
    pub x: f64,
    pub y: f64,
    /// The slot's place on screen now.
    pub vx: f64,
    pub vy: f64,
    pub w: f64,
    pub h: f64,
    /// The scroll box's content height, which picks the native scroll view
    /// the card rides in.
    pub scroll_height: f64,
    /// The visible part of the scroll box on screen, top and bottom, for
    /// example above a fixed tab bar. Android clips the card to it, because
    /// its card sits over the whole page. iOS needs neither: its card rides in
    /// the scroll box, under anything fixed on top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_top: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_bottom: Option<f64>,
    /// The words on the ad label, in the player's language. Google requires a
    /// visible ad label, so a slot without one shows "Ad".
    #[serde(default = "ad")]
    pub label: String,
    /// Colours as CSS `rgb()` text.
    pub ink: String,
    pub muted: String,
    pub button: String,
    pub edge: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_reads_from_the_page_names() {
        let slot: Slot = serde_json::from_str(
            r#"{"unitId":"u","x":0,"y":900,"vx":16,"vy":400,"w":343,"h":140,"scrollHeight":1200,
               "ink":"rgb(43, 26, 16)","muted":"rgb(125, 96, 74)","button":"rgb(255, 250, 242)","edge":"rgb(0, 0, 0)"}"#,
        )
        .unwrap();
        assert!((slot.y - 900.0).abs() < f64::EPSILON);
        assert!((slot.h - 140.0).abs() < f64::EPSILON);
        assert!((slot.scroll_height - 1200.0).abs() < f64::EPSILON);
        assert!(slot.clip_top.is_none());
        assert_eq!(slot.label, "Ad");
    }

    #[test]
    fn a_slot_reads_the_visible_part_of_its_scroll_box() {
        let slot: Slot = serde_json::from_str(
            r#"{"unitId":"u","x":0,"y":900,"vx":16,"vy":400,"w":343,"h":140,"scrollHeight":1200,
               "clipTop":0,"clipBottom":590,
               "ink":"","muted":"","button":"","edge":""}"#,
        )
        .unwrap();
        assert_eq!(slot.clip_top, Some(0.0));
        assert_eq!(slot.clip_bottom, Some(590.0));
    }

    #[test]
    fn a_slot_carries_the_app_s_words_for_the_ad_label() {
        let slot: Slot = serde_json::from_str(
            r#"{"unitId":"u","x":0,"y":0,"vx":0,"vy":0,"w":1,"h":1,"scrollHeight":1,
               "label":"Quảng cáo","ink":"","muted":"","button":"","edge":""}"#,
        )
        .unwrap();
        assert_eq!(slot.label, "Quảng cáo");
    }
}
