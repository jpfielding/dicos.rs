//! Layout checks at the review sizes: nothing may be clipped by a panel edge or
//! run off the screen, in any state, and the compact flows stay reachable.

use super::layout::{Layout, PhoneTab, SlicePage, VolumePage};
use super::{draw_ui, theme};
use crate::state::UiState;
use crate::transfer::TransferPreset;
use crate::volume::{ThreatBox, Volume};

const DESKTOP_SIZES: [egui::Vec2; 2] = [egui::vec2(1440.0, 900.0), egui::vec2(1100.0, 720.0)];
const PHONE_SIZES: [egui::Vec2; 2] = [egui::vec2(393.0, 852.0), egui::vec2(852.0, 393.0)];

fn run(ctx: &egui::Context, state: &mut UiState, size: egui::Vec2) -> egui::FullOutput {
    let mut output = None;
    // Panels size themselves over the first frames; judge the settled layout.
    for _ in 0..4 {
        output = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ctx| draw_ui(ctx, state),
        ));
    }
    output.unwrap()
}

/// A study shaped like the review sample: 680 × 440 × 200 mm, a long name, a
/// slice image and two detections (one with a long name).
fn study(ctx: &egui::Context) -> UiState {
    let mut state = UiState::new(crate::camera::Camera::default());
    state.library.loaded_path = Some(std::path::PathBuf::from(
        "backpack+purse_fishtank+goldfish_THREAT-knife_20260227-191512",
    ));
    let threat = |name: &str, color| ThreatBox {
        name: name.into(),
        confidence: Some(0.93),
        min: [1, 1, 1],
        max: [4, 4, 4],
        color,
        enabled: true,
    };
    state.library.volumes.push(crate::app::VolumeLayer {
        name: "ct".into(),
        volume: Volume {
            data: vec![0; 34 * 22 * 10],
            dim_x: 34,
            dim_y: 22,
            dim_z: 10,
            window_center: 238.0,
            window_width: 2524.0,
            rescale_intercept: 0.0,
            rescale_slope: 1.0,
            signed_pixels: false,
            modality: "CT".into(),
            voxel_spacing: [20.0; 3],
            threats: vec![
                threat("Knife", [255, 80, 80]),
                threat("Lithium battery pack, oversized", [255, 200, 60]),
            ],
        },
    });
    state.library.active_3d_index = Some(0);
    state.library.selected_threat = Some(0);
    state.slice_view.orientation = crate::slice_view::Orientation::Coronal;
    state.slice_view.max_slices = 22;
    state.slice_view.slice_index = 11;
    state.slice_view.window_center = 238.0;
    state.slice_view.window_width = 2524.0;
    state.settings.window_center = 238.0;
    state.settings.window_width = 2524.0;
    state.slice_texture = Some(ctx.load_texture(
        "slice",
        egui::ColorImage::filled([34, 10], egui::Color32::BLACK),
        egui::TextureOptions::NEAREST,
    ));
    state
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    ctx
}

/// Every piece of text and every drawn control edge that a clip rectangle or
/// the screen would cut. Images are exempt: a zoomed slice is meant to pan.
fn clipped(output: &egui::FullOutput, size: egui::Vec2) -> Vec<String> {
    fn visit(shape: &egui::Shape, clip: egui::Rect, screen: egui::Rect, found: &mut Vec<String>) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, screen, found);
                }
            }
            egui::Shape::Text(text) => {
                // The galley's own rect accounts for right-aligned text.
                let rect = text.galley.rect.translate(text.pos.to_vec2());
                if rect.width() > 0.0
                    && !(clip.expand(1.0).contains_rect(rect)
                        && screen.expand(0.5).contains_rect(rect))
                {
                    found.push(format!("text {:?} at {rect:?}", text.galley.job.text));
                }
            }
            egui::Shape::Rect(drawn) if drawn.brush.is_none() => {
                let rect = drawn.rect;
                let visible = drawn.fill != egui::Color32::TRANSPARENT
                    || drawn.stroke.color != egui::Color32::TRANSPARENT;
                if visible
                    && rect.is_positive()
                    && !(clip.expand(1.5).contains_rect(rect)
                        && screen.expand(1.5).contains_rect(rect))
                {
                    found.push(format!("rect at {rect:?} (clip {clip:?})"));
                }
            }
            _ => {}
        }
    }
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    let mut found = Vec::new();
    for clipped in &output.shapes {
        visit(&clipped.shape, clipped.clip_rect, screen, &mut found);
    }
    found
}

fn assert_fits(what: &str, ctx: &egui::Context, state: &mut UiState, size: egui::Vec2) {
    let output = run(ctx, state, size);
    let found = clipped(&output, size);
    assert!(found.is_empty(), "{what} at {size:?}: {found:#?}");
}

#[test]
fn desktop_views_fit_without_clipping() {
    for size in DESKTOP_SIZES {
        for preset in [TransferPreset::Monochrome, TransferPreset::Default] {
            for (name, focus, expanded, cutaway) in [
                ("workspace", false, false, false),
                ("cutaway", false, false, true),
                ("focus volume", true, false, false),
                ("focus volume with cutaway", true, false, true),
                ("focus slice", false, true, false),
            ] {
                let ctx = context();
                let mut state = study(&ctx);
                state.transfer.preset = preset;
                state.focus_volume = focus;
                state.slice_expanded = expanded;
                state.cutaway = cutaway;
                assert_fits(&format!("{name} / {preset:?}"), &ctx, &mut state, size);
                assert_eq!(state.layout, Layout::Desktop);
            }
        }
        let ctx = context();
        let mut empty = UiState::new(crate::camera::Camera::default());
        assert_fits("first use", &ctx, &mut empty, size);
    }
}

#[test]
fn phone_views_fit_without_clipping() {
    for size in PHONE_SIZES {
        // Every page of both controls sheets, in the tab view and in focus.
        for page in VolumePage::ALL {
            for (focus, adjust) in [(false, false), (true, true)] {
                let ctx = context();
                let mut state = study(&ctx);
                state.phone_tab = PhoneTab::View3d;
                state.volume_page = page;
                state.focus_volume = focus;
                state.focus_adjust_open = adjust;
                state.cutaway = true;
                state.transfer.preset = TransferPreset::Default;
                assert_fits(
                    &format!("3D {page:?} focus={focus}"),
                    &ctx,
                    &mut state,
                    size,
                );
            }
        }
        for page in [SlicePage::Slice, SlicePage::Image] {
            for (focus, adjust) in [(false, false), (true, false), (true, true)] {
                let ctx = context();
                let mut state = study(&ctx);
                state.phone_tab = PhoneTab::Slice;
                state.slice_page = page;
                state.slice_expanded = focus;
                state.focus_adjust_open = adjust;
                assert_fits(
                    &format!("2D {page:?} focus={focus}"),
                    &ctx,
                    &mut state,
                    size,
                );
            }
        }
        // The material-band editor, the pages, a collapsed sheet, first use.
        let ctx = context();
        let mut state = study(&ctx);
        state.phone_tab = PhoneTab::View3d;
        state.volume_page = VolumePage::Display;
        state.transfer.preset = TransferPreset::Default;
        state.bands_open = true;
        assert_fits("material bands", &ctx, &mut state, size);
        for tab in [PhoneTab::Study, PhoneTab::Detections] {
            let ctx = context();
            let mut state = study(&ctx);
            state.phone_tab = tab;
            assert_fits(&format!("{tab:?}"), &ctx, &mut state, size);
        }
        let ctx = context();
        let mut state = study(&ctx);
        state.phone_tab = PhoneTab::View3d;
        state.sheet_open = false;
        assert_fits("collapsed sheet", &ctx, &mut state, size);
        let ctx = context();
        let mut empty = UiState::new(crate::camera::Camera::default());
        assert_fits("first use", &ctx, &mut empty, size);
    }
}

#[test]
fn phone_sheets_keep_the_view_the_same_size_on_every_page() {
    for size in PHONE_SIZES {
        let ctx = context();
        let mut state = study(&ctx);
        state.phone_tab = PhoneTab::View3d;
        run(&ctx, &mut state, size);
        let first = state.viewport;
        assert!(
            first.height() >= 240.0 && first.width() >= 300.0,
            "{first:?}"
        );
        for page in VolumePage::ALL {
            state.volume_page = page;
            run(&ctx, &mut state, size);
            assert_eq!(state.viewport, first, "{page:?} at {size:?}");
        }
        state.phone_tab = PhoneTab::Slice;
        for page in [SlicePage::Slice, SlicePage::Image] {
            state.slice_page = page;
            run(&ctx, &mut state, size);
            assert_eq!(state.viewport, first, "{page:?} at {size:?}");
        }
    }
}

#[test]
fn the_volume_is_on_screen_only_where_the_layout_shows_it() {
    let ctx = context();
    let mut state = study(&ctx);
    run(&ctx, &mut state, DESKTOP_SIZES[0]);
    assert!(state.volume_visible());
    state.slice_expanded = true;
    assert!(!state.volume_visible(), "focus slice covers the 3D view");
    state.slice_expanded = false;

    run(&ctx, &mut state, PHONE_SIZES[0]);
    for (tab, visible) in [
        (PhoneTab::View3d, true),
        (PhoneTab::Slice, false),
        (PhoneTab::Study, false),
        (PhoneTab::Detections, false),
    ] {
        state.phone_tab = tab;
        assert_eq!(state.volume_visible(), visible, "{tab:?}");
    }
    state.focus_volume = true;
    assert!(state.volume_visible(), "focus volume shows it from any tab");
}

#[test]
fn long_names_keep_both_ends() {
    let ctx = context();
    let name = "backpack+purse_fishtank+goldfish_THREAT-knife_20260227-191512";
    let mut shown = String::new();
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::FontId::proportional(13.0);
            shown = theme::elide_middle(ui.painter(), name, font.clone(), 200.0);
            let width = ui
                .painter()
                .layout_no_wrap(shown.clone(), font.clone(), theme::TEXT)
                .size()
                .x;
            assert!(width <= 200.0, "{shown:?} is {width} pt wide");
            assert_eq!(theme::elide_middle(ui.painter(), name, font, 2000.0), name);
        });
    });
    assert!(shown.starts_with("backpack"), "{shown:?}");
    assert!(shown.ends_with("20260227-191512"), "{shown:?}");
    assert!(shown.contains('…'), "{shown:?}");
}
