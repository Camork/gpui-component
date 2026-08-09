//! Flame graph example: a pure-UI multi-track flame chart component test app.
//!
//! Demonstrates integrating `gpui_component::flamegraph` with a custom toolbar and fake data:
//! - wheel (no modifier) pans the shared time axis
//! - ctrl+wheel zooms, anchored at the cursor
//! - drag draws a box-select that *shrinks* the view to that time range
//! - double-click on the canvas resets to the full trace
//! - click a frame to select (highlight only), hover for a tooltip
//! - click a track header to collapse/expand it
//! - search box dims non-matching frames in real time

mod fake_data;
mod view;

use gpui::*;
use gpui_component::Root;
use gpui_component_assets::Assets;
use view::AppView;

actions!(Quit, [Quit]);

fn main() {
    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        // This must be called before using any GPUI Component features.
        gpui_component::init(cx);

        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1100.), px(720.)), cx)),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| AppView::new(window, cx));
                // The first level on the window should be a Root.
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use gpui_component::flamegraph::{FlameFrame, FlameGraphState, Session, Track, Viewport, flatten};

    fn tiny_session() -> Session {
        let root = FlameFrame::new(
            "a",
            std::time::Duration::from_secs_f64(0.0),
            std::time::Duration::from_secs_f64(1.0),
        )
        .with_children(vec![FlameFrame::new(
            "b",
            std::time::Duration::from_secs_f64(0.2),
            std::time::Duration::from_secs_f64(0.4),
        )]);
        Session {
            tracks: vec![Track {
                name: "t".into(),
                collapsed: false,
                roots: vec![root],
            }],
        }
    }

    #[test]
    fn track_geometry_is_a_pure_function_of_collapse_state() {
        let session = tiny_session();
        let flat = flatten(&session);
        let depths: Vec<usize> = flat.tracks.iter().map(|t| t.max_depth).collect();
        let collapsed = vec![false];

        let (hy, ry, rh) = FlameGraphState::track_layout_y(&collapsed, &depths, 0);
        assert_eq!(hy, 0.0);
        assert_eq!(ry, 28.0);
        assert_eq!(rh, (depths[0] + 1) as f32 * 18.0);
        assert_eq!(rh, 2.0 * 18.0);

        // collapsed: no row area at all
        let collapsed = vec![true];
        let (hy, ry, rh) = FlameGraphState::track_layout_y(&collapsed, &depths, 0);
        assert_eq!((hy, ry), (0.0, 28.0));
        assert_eq!(rh, 0.0);
    }

    #[test]
    fn measure_map_converts_both_drag_edges() {
        let vp = Viewport::full(10.0);
        let width = 1000.0;
        let a = vp.x_to_time(100.0, width);
        let b = vp.x_to_time(300.0, width);
        assert_eq!((a, b), (1.0, 3.0));
        assert_eq!(a.min(b), 1.0);
        assert_eq!(b.min(a), 1.0);
        assert_eq!(vp.duration(), 10.0);
    }
}
