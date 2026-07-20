//! Custom transfer-band range slider widget.

use crate::transfer::ColorBand;

use super::{band_color, clamp_band_thresholds};

pub(crate) fn draw_band_range_slider(ui: &mut egui::Ui, bands: &mut [ColorBand]) -> bool {
    if bands.len() < 2 {
        ui.label("Need at least two bands");
        return false;
    }

    let max_density = bands.last().map(|b| b.threshold as f32).unwrap_or(30000.0);
    let max_density = max_density.max(1.0);

    let desired_size = egui::vec2(ui.available_width().max(180.0), 78.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    let track_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 8.0, rect.bottom() - 28.0),
        egui::pos2(rect.right() - 8.0, rect.bottom() - 16.0),
    );

    let to_x =
        |value: f32| -> f32 { track_rect.left() + (value / max_density) * track_rect.width() };
    let to_value = |x: f32| -> i32 {
        let t = ((x - track_rect.left()) / track_rect.width()).clamp(0.0, 1.0);
        (t * max_density).round() as i32
    };

    let handle_count = bands.len() - 1;
    let id = response.id.with("band_handles");

    if (response.drag_started() || response.clicked()) && response.interact_pointer_pos().is_some()
    {
        let pointer_x = response.interact_pointer_pos().unwrap().x;
        let mut nearest_idx = 0usize;
        let mut nearest_dist = f32::INFINITY;
        for (i, band) in bands.iter().take(handle_count).enumerate() {
            let hx = to_x(band.threshold as f32);
            let dist = (pointer_x - hx).abs();
            if dist < nearest_dist {
                nearest_dist = dist;
                nearest_idx = i;
            }
        }
        ui.memory_mut(|m| m.data.insert_temp(id, nearest_idx));
    }

    if response.drag_stopped() {
        ui.memory_mut(|m| {
            m.data.remove::<usize>(id);
        });
    }

    let mut changed = false;
    if (response.dragged() || response.clicked()) && response.interact_pointer_pos().is_some() {
        if let Some(active_idx) = ui.memory(|m| m.data.get_temp::<usize>(id)) {
            let pointer_x = response.interact_pointer_pos().unwrap().x;
            let mut value = to_value(pointer_x);

            let lower = if active_idx == 0 {
                0
            } else {
                bands[active_idx - 1].threshold as i32
            };
            let upper = if active_idx + 1 < handle_count {
                bands[active_idx + 1].threshold as i32
            } else {
                max_density as i32
            };
            value = value.clamp(lower, upper);

            if bands[active_idx].threshold != value as u16 {
                bands[active_idx].threshold = value as u16;
                changed = true;
            }
        }
    }

    if changed {
        clamp_band_thresholds(bands);
        response.mark_changed();
    }

    painter.rect_filled(track_rect, 3.0, ui.visuals().widgets.inactive.bg_fill);

    for i in 0..bands.len() {
        let start = if i == 0 { 0u16 } else { bands[i - 1].threshold };
        let end = if i == bands.len() - 1 {
            max_density as u16
        } else {
            bands[i].threshold
        };
        let sx = to_x(start as f32);
        let ex = to_x(end as f32);
        if ex <= sx {
            continue;
        }

        let mut color = band_color(&bands[i]);
        if bands[i].is_transparent {
            color = color.gamma_multiply(0.25);
        }

        let seg_rect = egui::Rect::from_min_max(
            egui::pos2(sx, track_rect.top()),
            egui::pos2(ex, track_rect.bottom()),
        );
        painter.rect_filled(seg_rect, 2.0, color);

        let seg_width = ex - sx;
        if seg_width > 28.0 {
            painter.text(
                egui::pos2((sx + ex) * 0.5, track_rect.bottom() + 6.0),
                egui::Align2::CENTER_TOP,
                bands[i].name,
                egui::FontId::proportional(10.0),
                ui.visuals().text_color(),
            );
        }
    }

    for (i, band) in bands.iter().enumerate().take(handle_count) {
        let hx = to_x(band.threshold as f32);
        let center = egui::pos2(hx, track_rect.center().y);
        let active = ui.memory(|m| m.data.get_temp::<usize>(id)) == Some(i) && response.dragged();
        let stroke_color = if active {
            ui.visuals().selection.stroke.color
        } else {
            ui.visuals().widgets.inactive.fg_stroke.color
        };

        painter.circle_filled(center, 6.0, ui.visuals().extreme_bg_color);
        painter.circle_stroke(center, 6.0, egui::Stroke::new(1.5_f32, stroke_color));
        painter.text(
            egui::pos2(hx, track_rect.top() - 4.0),
            egui::Align2::CENTER_BOTTOM,
            format!("{}", band.threshold),
            egui::FontId::monospace(10.0),
            ui.visuals().text_color(),
        );
    }

    changed
}
