# Roxel UI overhaul review

Date: 2026-11-05. PR: [UI polish overhaul](https://github.com/jpfielding/dicos.rs/pull/24).

## Scope and evidence

Three independent reviewers covered visual design, codec correctness, and the DICOS API. Each also inspected actual Roxel screenshots and assigned its own visual score. Reviewers were not given the acceptance score. Visual review remains in progress; the latest approved screenshot set and scores will be recorded here when complete.

The screenshots are readbacks of the native application’s GPU rendering, including its egui interface and loaded repository fixtures. They are not mockups or edited image composites. The README image is the same workspace capture included in the review set. Reproduction commands and capture options are in the [viewer README](../../crates/roxel/README.md#reproducible-screenshots).

## Improvements driven by review

- A bounded 3D viewport, automatic fitting, physical XYZ proportions, and consistent slice aspect ratios.
- A quieter dark workspace with clearer hierarchy, aligned controls and progressive disclosure.
- Focused volume inspection and expanded slice inspection, with explicit return paths and retained study identity.
- A linked voxel-center slice plane, reversible cutaway, retained-side annotation and zoom-aware physical scale bar.
- Windowing controls that use useful study ranges while retaining wider numeric entry.
- Corrected sampling-distance opacity, physical lighting normals, and consistent rescale-intercept handling in automatic windows, slices, projections and 3D.
- Material edits survive palette/layer changes; newly selected volumes initialize their own window values.
- Regression coverage for physical geometry, camera fitting, uniform layout, transfer preservation, nonzero intercepts, long identifiers, focus layout changes, modal navigation and Escape priority.

## What the images do and do not prove

Runtime captures exercised the Metal GPU shader and UI rendering at standard and minimum window sizes. Pointer/keyboard regression tests feed real egui events and verify their specific state transitions. Native OS interaction automation was unavailable: the app-control connector could not attach to the executable/bundle, and local accessibility/screen-capture permission was unavailable. No native end-to-end usability claim is made.

A separate read-only probe found only 13 distinct raw intensities in `bag_ct.dcs` and 10 in the tall study. Thin, one- or two-voxel shells and stepped boundaries exist in those source volumes. A 144-ray CPU scalar-opacity comparison between half-voxel and one-sixteenth-voxel sampling found mean absolute accumulated-alpha differences of 0.000381 and 0.000195 respectively. This isolates scalar integration; it does not prove shaded GPU RGB convergence. The source data were not smoothed to improve screenshots.

Physical dimensions use the loader’s voxel-spacing values; missing spacing currently defaults to one unit per voxel. A physical scale bar is display context, not a measurement or calibration certification. Signed samples and non-unit rescale slopes remain unsupported. Extremely long rays use coarser sampling to fit the shader step budget, and opacity still uses the existing 98% early-termination approximation.

## Correctness review follow-ups

The [codec review](codec-review.md) and [DICOS API review](dicos-api-review.md) preserve independently reproduced findings and their limitations. Codec conformance and general DICOS round-trip correctness are not certified by the UI screenshots or passing existing tests. The API review’s 2D/3D intercept inconsistency was fixed here for supported unsigned, unit-slope inputs; the other parser/writer and ingestion findings remain open.

## Screenshot set

- [Workspace / README snapshot](../images/roxel-workspace.png)
- [Minimum-size workspace](../images/roxel-compact.png)
- [Focused volume](../images/roxel-focus.png)
- [Expanded slice inspection](../images/roxel-slice.png)
- [Minimum-size cutaway](../images/roxel-cutaway.png)
- [Empty state](../images/roxel-empty.png)

Validation: `cargo test --workspace --all-features` passed before the final visual refinements. After those refinements, `cargo test -p roxel` passed 71 tests, `cargo clippy -p roxel --all-targets -- -D warnings` passed, and formatting/whitespace checks passed. The screenshots above were recaptured after the final camera-fit correction and visual pass.
