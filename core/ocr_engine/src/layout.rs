use serde::{Deserialize, Serialize};

use crate::geometry::BBox;
use crate::table::TableStructure;

/// PP-StructureV2 layout categories. The default label map matches the English
/// model; a different model can pass its own `classify_label`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegionKind {
    Text,
    Title,
    List,
    Table,
    Figure,
    Equation,
    Header,
    Footer,
    Caption,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub kind: RegionKind,
    pub bbox: BBox,
    pub confidence: f32,
    /// Recognized text for text-like regions; `None` until recognition runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Reconstructed cells for table regions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<TableStructure>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageLayout {
    pub width: u32,
    pub height: u32,
    pub regions: Vec<Region>,
}

/// Default PP-StructureV2 label map (english layout model).
pub fn classify_label(label: i64) -> Option<RegionKind> {
    match label {
        1 => Some(RegionKind::Text),
        2 => Some(RegionKind::Title),
        3 => Some(RegionKind::List),
        4 => Some(RegionKind::Table),
        5 => Some(RegionKind::Figure),
        6 => Some(RegionKind::Equation),
        8 => Some(RegionKind::Header),
        9 => Some(RegionKind::Footer),
        10 => Some(RegionKind::Caption),
        _ => None,
    }
}

/// PicoDet PP-StructureV2 layout label map (0-based): text, title, list, table,
/// figure. The downloaded `picodet_lcnet_x1_0_fgd_layout` export uses this.
pub fn classify_pico_label(label: i64) -> Option<RegionKind> {
    match label {
        0 => Some(RegionKind::Text),
        1 => Some(RegionKind::Title),
        2 => Some(RegionKind::List),
        3 => Some(RegionKind::Table),
        4 => Some(RegionKind::Figure),
        _ => None,
    }
}

/// Builds regions from a layout model's `(label, score, bbox)` output, dropping
/// labels the model emits that we have no use for (background, reference, ...).
pub fn from_detection(
    width: u32,
    height: u32,
    labels: &[i64],
    scores: &[f32],
    boxes: &[BBox],
) -> PageLayout {
    let mut regions = Vec::new();
    for (index, &label) in labels.iter().enumerate() {
        let Some(kind) = classify_label(label) else {
            continue;
        };
        let score = scores.get(index).copied().unwrap_or(0.0);
        let bbox = boxes
            .get(index)
            .copied()
            .unwrap_or_default()
            .clamp(width, height);
        regions.push(Region {
            kind,
            bbox,
            confidence: score,
            text: None,
            table: None,
        });
    }
    sort_reading_order(&mut regions, false);
    PageLayout {
        width,
        height,
        regions,
    }
}

/// Orders regions top-to-bottom, then left-to-right (LTR) or right-to-left
/// (RTL). Regions whose vertical centers are within `line_tolerance` of each
/// other share a line, so a title beside a figure is not reordered vertically.
pub fn sort_reading_order(regions: &mut [Region], rtl: bool) {
    let line_tolerance = 12.0;
    regions.sort_by(|a, b| {
        let a_y = a.bbox.center().y;
        let b_y = b.bbox.center().y;
        let a_line = (a_y / line_tolerance).round() as i64;
        let b_line = (b_y / line_tolerance).round() as i64;
        if a_line != b_line {
            return a_line.cmp(&b_line);
        }
        if rtl {
            b.bbox
                .center()
                .x
                .partial_cmp(&a.bbox.center().x)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            a.bbox
                .center()
                .x
                .partial_cmp(&b.bbox.center().x)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_label_map_matches_pp_structure() {
        assert_eq!(classify_label(1), Some(RegionKind::Text));
        assert_eq!(classify_label(2), Some(RegionKind::Title));
        assert_eq!(classify_label(4), Some(RegionKind::Table));
        assert_eq!(classify_label(0), None);
    }

    #[test]
    fn pico_label_map_is_zero_based() {
        assert_eq!(classify_pico_label(0), Some(RegionKind::Text));
        assert_eq!(classify_pico_label(1), Some(RegionKind::Title));
        assert_eq!(classify_pico_label(3), Some(RegionKind::Table));
        assert_eq!(classify_pico_label(5), None);
    }

    #[test]
    fn detection_drops_unknown_labels_and_clamps_boxes() {
        let layout = from_detection(
            100,
            100,
            &[1, 0, 4],
            &[0.9, 0.8, 0.7],
            &[
                BBox::new(-5.0, 0.0, 50.0, 20.0),
                BBox::new(0.0, 0.0, 1.0, 1.0),
                BBox::new(10.0, 30.0, 90.0, 50.0),
            ],
        );
        assert_eq!(layout.regions.len(), 2);
        assert_eq!(layout.regions[0].kind, RegionKind::Text);
        assert_eq!(layout.regions[0].bbox.x0, 0.0);
        assert_eq!(layout.regions[1].kind, RegionKind::Table);
    }

    #[test]
    fn reading_order_is_top_to_bottom_then_left_to_right() {
        let mut regions = vec![
            region(RegionKind::Text, 50.0, 10.0, 60.0, 20.0),
            region(RegionKind::Text, 10.0, 80.0, 20.0, 90.0),
            region(RegionKind::Title, 10.0, 10.0, 20.0, 20.0),
        ];
        sort_reading_order(&mut regions, false);
        assert_eq!(regions[0].kind, RegionKind::Title);
        assert_eq!(regions[1].kind, RegionKind::Text);
        assert_eq!(regions[2].kind, RegionKind::Text);
        assert!(regions[0].bbox.y0 < regions[2].bbox.y0);
        assert!(regions[1].bbox.y0 < regions[2].bbox.y0);
    }

    #[test]
    fn reading_order_flips_for_rtl() {
        let mut regions = vec![
            region(RegionKind::Text, 10.0, 10.0, 10.0, 10.0),
            region(RegionKind::Text, 80.0, 10.0, 10.0, 10.0),
        ];
        sort_reading_order(&mut regions, true);
        assert!(regions[0].bbox.x0 > regions[1].bbox.x0);
    }

    fn region(kind: RegionKind, x0: f32, y0: f32, x1: f32, y1: f32) -> Region {
        Region {
            kind,
            bbox: BBox::new(x0, y0, x1, y1),
            confidence: 1.0,
            text: None,
            table: None,
        }
    }
}
