//! PicoDet (PP-StructureV2 layout) post-processing.
//!
//! The exported ONNX emits, per FPN level, a score tensor `[N, C]` and a box
//! tensor `[N, 32]`. The 32 values are four sides × eight distance bins; each
//! side is decoded with a softmax over the bins, weighted by the bin index and
//! the stride. Centers come from the feature-map grid.

use crate::geometry::BBox;

pub struct PicoDetection {
    pub label: i64,
    pub score: f32,
    pub bbox: BBox,
}

const STRIDES: [i64; 4] = [8, 16, 32, 64];
const REG_MAX: i64 = 7;

/// Decodes all four FPN levels and returns the surviving detections, already
/// scaled to the original image.
// Every argument is a distinct axis of the tensor shape or a decode constant;
// bundling them into a struct would only move the same list one level down.
#[allow(clippy::too_many_arguments)]
pub fn decode_layout(
    scores: &[(&[i64], &[f32])],
    boxes: &[(&[i64], &[f32])],
    input_w: u32,
    input_h: u32,
    orig_w: u32,
    orig_h: u32,
    score_threshold: f32,
    nms_threshold: f32,
    keep_top_k: usize,
) -> Vec<PicoDetection> {
    let mut all = Vec::new();
    for (level, (&(score_shape, score_data), &(box_shape, box_data))) in
        scores.iter().zip(boxes.iter()).enumerate()
    {
        let stride = STRIDES.get(level).copied().unwrap_or(8);
        let mut detections = decode_level(
            score_data,
            score_shape,
            box_data,
            box_shape,
            stride,
            input_w,
            input_h,
            orig_w,
            orig_h,
            score_threshold,
        );
        all.append(&mut detections);
    }
    nms(&mut all, nms_threshold, keep_top_k);
    all
}

// Tensor slices and their shapes are the natural signature here: the level
// decoder reads both without owning either.
#[allow(clippy::too_many_arguments)]
fn decode_level(
    scores: &[f32],
    score_shape: &[i64],
    box_dist: &[f32],
    box_shape: &[i64],
    stride: i64,
    input_w: u32,
    input_h: u32,
    orig_w: u32,
    orig_h: u32,
    score_threshold: f32,
) -> Vec<PicoDetection> {
    let classes = (*score_shape.last().unwrap_or(&1)).max(1) as usize;
    let rows = scores.len() / classes;
    let bins = (REG_MAX + 1) as usize;
    let fm_w = input_w.div_ceil(stride as u32) as usize;

    let mut out = Vec::new();
    for row in 0..rows {
        let score_base = row * classes;
        if score_base + classes > scores.len() {
            break;
        }
        let (label, score) = scores[score_base..score_base + classes]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(index, &value)| (index as i64, value))
            .unwrap_or((0, 0.0));
        if score < score_threshold {
            continue;
        }

        let box_base = row * bins * 4;
        if box_base + bins * 4 > box_dist.len()
            || box_shape.last().copied().unwrap_or(0) != (bins * 4) as i64
        {
            break;
        }

        let mut distance = [0.0f32; 4];
        for (side, slot) in distance.iter_mut().enumerate() {
            let side_base = box_base + side * bins;
            let max = box_dist[side_base..side_base + bins]
                .iter()
                .copied()
                .fold(f32::MIN, f32::max);
            let denominator: f32 = box_dist[side_base..side_base + bins]
                .iter()
                .map(|value| (value - max).exp())
                .sum();
            let numerator: f32 = box_dist[side_base..side_base + bins]
                .iter()
                .enumerate()
                .map(|(bin, value)| bin as f32 * (value - max).exp())
                .sum();
            *slot = if denominator > 0.0 {
                numerator / denominator * stride as f32
            } else {
                0.0
            };
        }

        let center_x = (row % fm_w) as f32 + 0.5;
        let center_y = (row / fm_w) as f32 + 0.5;
        let center_x = center_x * stride as f32;
        let center_y = center_y * stride as f32;

        let bbox = BBox::new(
            (center_x - distance[0]) / input_w as f32 * orig_w as f32,
            (center_y - distance[1]) / input_h as f32 * orig_h as f32,
            (center_x + distance[2]) / input_w as f32 * orig_w as f32,
            (center_y + distance[3]) / input_h as f32 * orig_h as f32,
        );
        out.push(PicoDetection {
            label,
            score,
            bbox: bbox.clamp(orig_w, orig_h),
        });
    }
    out
}

/// Class-wise greedy NMS, keeping at most `keep_top_k` detections.
pub fn nms(detections: &mut Vec<PicoDetection>, iou_threshold: f32, keep_top_k: usize) {
    detections.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<PicoDetection> = Vec::new();
    for candidate in detections.drain(..) {
        if kept.len() >= keep_top_k {
            break;
        }
        let overlap = kept.iter().any(|other| {
            other.label == candidate.label && other.bbox.iou(&candidate.bbox) > iou_threshold
        });
        if !overlap {
            kept.push(candidate);
        }
    }
    *detections = kept;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_one_hot_distance_bin_decodes_to_its_index_times_stride() {
        // 5051 locations on a 100x76 grid; only location 5050 fires. Class 1
        // wins; side 0 is one-hot at bin 4, the other sides at bin 1.
        let rows = 3831usize;
        let mut scores = vec![0.01f32; rows * 5];
        let mut boxes = vec![0.0f32; rows * 32];
        let row = 3830usize;
        scores[row * 5 + 1] = 0.9;
        for side in 0..4 {
            let bin = if side == 0 { 4 } else { 1 };
            boxes[row * 32 + side * 8 + bin] = 10.0;
        }
        let score_shape = [rows as i64, 5];
        let box_shape = [rows as i64, 32];
        let detections = decode_level(
            &scores,
            &score_shape,
            &boxes,
            &box_shape,
            8,
            608,
            800,
            608,
            800,
            0.3,
        );
        assert_eq!(detections.len(), 1);
        let detection = &detections[0];
        assert_eq!(detection.label, 1);
        // grid is 76x100; location 3830 -> (x=30.5, y=50.5) * 8 = (244, 404);
        // side distances = 32, 8, 8, 8
        assert!((detection.bbox.x0 - (244.0 - 32.0)).abs() < 1e-2);
        assert!((detection.bbox.y0 - (404.0 - 8.0)).abs() < 1e-2);
        assert!((detection.bbox.x1 - (244.0 + 8.0)).abs() < 1e-2);
    }

    #[test]
    fn nms_keeps_the_highest_scoring_box_per_overlap_group() {
        let mut detections = vec![
            PicoDetection {
                label: 0,
                score: 0.9,
                bbox: BBox::new(0.0, 0.0, 10.0, 10.0),
            },
            PicoDetection {
                label: 0,
                score: 0.8,
                bbox: BBox::new(1.0, 1.0, 11.0, 11.0),
            },
            PicoDetection {
                label: 1,
                score: 0.7,
                bbox: BBox::new(1.0, 1.0, 11.0, 11.0),
            },
        ];
        nms(&mut detections, 0.5, 10);
        assert_eq!(detections.len(), 2);
        assert_eq!(detections[0].score, 0.9);
        assert_eq!(detections[1].label, 1);
    }

    #[test]
    fn low_score_locations_are_dropped() {
        let scores = vec![0.1f32; 5];
        let boxes = vec![0.0f32; 32];
        let detections = decode_level(
            &scores,
            &[1, 5],
            &boxes,
            &[1, 32],
            8,
            608,
            800,
            608,
            800,
            0.3,
        );
        assert!(detections.is_empty());
    }
}
