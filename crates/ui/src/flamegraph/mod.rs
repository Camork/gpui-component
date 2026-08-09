//! Multi-track flame graph component for GPUI.
//!
//! Provides an interactive flame chart with zoom, pan, drag-to-measure,
//! track collapse, frame search, and a time-axis ruler. Built as a
//! reusable component on GPUI's native render pipeline.
//!
//! # Quick Start
//!
//! ```ignore
//! let state = cx.new(|_| FlameGraphState::new(session));
//! // In your Render impl:
//! FlameGraph::new(&state)
//! ```

mod canvas;
pub mod data;
pub mod format;
mod ruler;
pub mod source;
pub mod perfetto;
pub mod state;
pub mod style;
mod tooltip;
pub mod viewport;

pub use data::{FlameFrame, FlatFrame, FlatSession, Session, Track, TrackLayout, flatten};
pub use format::{TimeFormatConfig, TimeUnit, format_duration, format_tick};
pub use perfetto::{PerfettoDataSource, PerfettoQueryBuilder};
pub use source::{FlameDataSource, MemoryDataSource, QueryFrame, QueryResult, QueryTrack, TrackInfo};
pub use state::{FlameGraphState, FrameId};
pub use style::FlameGraphStyle;
pub use viewport::Viewport;

use gpui::{App, Entity, IntoElement, RenderOnce, Window, div, prelude::*, px};

use crate::scroll::{Scrollbar, ScrollbarShow};
use crate::ActiveTheme;

use self::canvas::FlameCanvas;
use self::ruler::RulerCanvas;

/// A flame graph component displaying multi-track profiling data.
///
/// Wraps the internal canvas, ruler, and scrollbar into a single
/// composable element. The caller provides a [`FlameGraphState`]
/// entity that owns all interactive state.
#[derive(IntoElement)]
pub struct FlameGraph {
    state: Entity<FlameGraphState>,
}

impl FlameGraph {
    /// Create a new flame graph component backed by the given state.
    pub fn new(state: &Entity<FlameGraphState>) -> Self {
        Self {
            state: state.clone(),
        }
    }
}

impl RenderOnce for FlameGraph {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let ruler_h = state.style.ruler_height;
        let scroll = state.scroll.clone();
        let h_scroll = state.h_scroll.clone();

        // Update content height for the scrollbar.
        let mut scroll_state = scroll.0.borrow_mut();
        scroll_state.content_h = state.content_height();
        drop(scroll_state);

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                // Flame area: canvas + scrollbar overlays
                div()
                    .flex_grow_1()
                    .relative()
                    .overflow_hidden()
                    .child(FlameCanvas { view: self.state.clone() })
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .w(px(12.0))
                            .child(
                                Scrollbar::vertical(&scroll)
                                    .scrollbar_show(ScrollbarShow::Scrolling),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right(px(12.0))
                            .bottom_0()
                            .h(px(16.0))
                            .child(
                                Scrollbar::horizontal(&h_scroll)
                                    .scrollbar_show(ScrollbarShow::Hover),
                            ),
                    ),
            )
            .child(
                // Ruler area
                div()
                    .h(px(ruler_h))
                    .w_full()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(RulerCanvas { view: self.state.clone() }),
            )
    }
}
