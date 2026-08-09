//! Data source abstraction for the flame graph component.
//!
//! [`FlameDataSource`] decouples the component from any specific storage
//! backend. The built-in [`MemoryDataSource`] wraps the in-memory
//! [`Session`]/[`FlatSession`] model for backward compatibility; production
//! callers can implement the trait on top of Perfetto, SQLite, or any other
//! query engine.

use gpui::Hsla;

use super::data::{FlatFrame, FlatSession, Session, flatten};

// ── query result types ──────────────────────────────────────────────────────

/// Metadata for a single track (independent of the current viewport).
#[derive(Clone, Debug)]
pub struct TrackInfo {
    pub name: String,
    pub max_depth: usize,
}

/// A single frame returned by a data-source query.
///
/// Unlike [`super::data::FlatFrame`], this includes a `matched` field so the
/// data source can handle search filtering server-side.
#[derive(Clone, Debug)]
pub struct QueryFrame {
    pub name: String,
    pub color: Hsla,
    pub row: usize,
    pub abs_start: f64,
    pub abs_end: f64,
    /// Whether this frame matches the current search query.
    pub matched: bool,
}

/// One track's worth of query results.
#[derive(Clone, Debug)]
pub struct QueryTrack {
    pub info: TrackInfo,
    pub frames: Vec<QueryFrame>,
}

/// The complete result of a viewport query.
#[derive(Clone, Debug)]
pub struct QueryResult {
    pub tracks: Vec<QueryTrack>,
}

// ── trait ────────────────────────────────────────────────────────────────────

/// Abstract data source for the flame graph component.
///
/// Implementations handle data loading, LOD aggregation, and search
/// filtering. The component only renders what the source provides.
///
/// # Contract
///
/// - `query_visible_frames` is called on every viewport or search change.
///   For in-memory sources this is cheap; async backends should implement
///   caching / debounce externally.
/// - Returned `QueryFrame` lists within each track **must** be sorted by
///   `abs_start` ascending. The renderer relies on this for binary search.
/// - `matched` must be set correctly for every frame; the component uses it
///   to dim non-matching frames and will **not** do its own string matching.
pub trait FlameDataSource {
    /// Query visible frames for the given time range and screen resolution.
    ///
    /// * `time_range` — `(start, end)` in seconds.
    /// * `screen_width_px` — pixel width of the canvas, so the source can
    ///   decide an appropriate aggregation granularity.
    /// * `search_query` — current search filter (empty = show all).
    fn query_visible_frames(
        &self,
        time_range: (f64, f64),
        screen_width_px: f32,
        search_query: &str,
    ) -> QueryResult;

    /// Total duration of the session in seconds.
    fn session_duration(&self) -> f64;

    /// Number of tracks.
    fn track_count(&self) -> usize;

    /// Metadata for a specific track.
    fn track_info(&self, index: usize) -> TrackInfo;

    /// Downcast support: allows callers to recover the concrete source type.
    fn as_any(&self) -> &dyn std::any::Any;
}

// ── in-memory implementation ────────────────────────────────────────────────

/// In-memory data source wrapping the existing [`Session`] / [`FlatSession`].
///
/// Suitable for small-to-medium datasets (< ~1 M frames). No LOD
/// aggregation: returns all frames whose time range intersects the query
/// window, with binary-search entry and sub-pixel filtering.
pub struct MemoryDataSource {
    session: Session,
    flat: FlatSession,
}

impl MemoryDataSource {
    /// Create a new in-memory source from a [`Session`].
    pub fn new(session: Session) -> Self {
        let flat = flatten(&session);
        Self { session, flat }
    }

    /// Replace the underlying session data.
    pub fn set_session(&mut self, session: Session) {
        self.flat = flatten(&session);
        self.session = session;
    }

    /// Access the raw session (e.g. for counting frames in the example).
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Access the flattened session (e.g. for track depth stats).
    pub fn flat(&self) -> &FlatSession {
        &self.flat
    }
}

impl FlameDataSource for MemoryDataSource {
    fn query_visible_frames(
        &self,
        time_range: (f64, f64),
        screen_width_px: f32,
        search_query: &str,
    ) -> QueryResult {
        let (vp_start, vp_end) = time_range;
        let search_lower = search_query.to_lowercase();
        let has_search = !search_query.is_empty();
        let px_per_sec = if (vp_end - vp_start).abs() > 1e-15 {
            screen_width_px as f64 / (vp_end - vp_start)
        } else {
            1.0
        };

        let tracks = self
            .flat
            .tracks
            .iter()
            .enumerate()
            .map(|(ti, track)| {
                let mut frames = Vec::new();

                for row_frames in &track.rows {
                    if row_frames.is_empty() {
                        continue;
                    }

                    // Binary search on this specific row to find the first frame that could be visible.
                    // A frame is visible if abs_end > vp_start AND abs_start < vp_end.
                    // Back up 1 frame on this row in case the preceding frame spans into [vp_start..vp_end].
                    let entry = match row_frames.binary_search_by(|f| {
                        f.abs_start
                            .partial_cmp(&vp_start)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }) {
                        Ok(i) => i,
                        Err(i) => i.saturating_sub(1),
                    };

                    let mut cluster_start: f64 = 0.0;
                    let mut cluster_end: f64 = 0.0;
                    let mut cluster_first: Option<&FlatFrame> = None;
                    let mut cluster_matched = false;

                    for f in &row_frames[entry..] {
                        // Past the viewport — no more visible frames on this row.
                        if f.abs_start >= vp_end {
                            break;
                        }
                        // Before the viewport — skip.
                        if f.abs_end <= vp_start {
                            continue;
                        }

                        let start_x = (f.abs_start - vp_start) * px_per_sec;
                        let end_x = (f.abs_end - vp_start) * px_per_sec;
                        let width_px = (end_x - start_x).max(0.1);

                        let matched = if has_search {
                            f.name.to_lowercase().contains(&search_lower)
                        } else {
                            true
                        };

                        if let Some(first) = cluster_first {
                            let active_end_x = (cluster_end - vp_start) * px_per_sec;
                            let gap_px = start_x - active_end_x;

                            // Micro-gap threshold: only merge if gap <= 0.2px (same sub-pixel column)
                            // AND both current cluster and frame are sub-pixel (width <= 1.0px)
                            if gap_px <= 0.2 && width_px <= 1.0 {
                                cluster_end = cluster_end.max(f.abs_end);
                                cluster_matched |= matched;
                                continue;
                            } else {
                                frames.push(QueryFrame {
                                    name: first.name.clone(),
                                    color: first.color,
                                    row: first.row,
                                    abs_start: cluster_start,
                                    abs_end: cluster_end,
                                    matched: cluster_matched,
                                });
                                cluster_first = None;
                            }
                        }

                        if width_px <= 1.0 {
                            cluster_first = Some(f);
                            cluster_start = f.abs_start;
                            cluster_end = f.abs_end;
                            cluster_matched = matched;
                        } else {
                            frames.push(QueryFrame {
                                name: f.name.clone(),
                                color: f.color,
                                row: f.row,
                                abs_start: f.abs_start,
                                abs_end: f.abs_end,
                                matched,
                            });
                        }
                    }

                    if let Some(first) = cluster_first {
                        frames.push(QueryFrame {
                            name: first.name.clone(),
                            color: first.color,
                            row: first.row,
                            abs_start: cluster_start,
                            abs_end: cluster_end,
                            matched: cluster_matched,
                        });
                    }
                }

                QueryTrack {
                    info: TrackInfo {
                        name: self
                            .session
                            .tracks
                            .get(ti)
                            .map(|t| t.name.clone())
                            .unwrap_or_default(),
                        max_depth: track.max_depth,
                    },
                    frames,
                }
            })
            .collect();

        QueryResult { tracks }
    }

    fn session_duration(&self) -> f64 {
        self.flat.session_end
    }

    fn track_count(&self) -> usize {
        self.flat.tracks.len()
    }

    fn track_info(&self, index: usize) -> TrackInfo {
        let track = &self.flat.tracks[index];
        TrackInfo {
            name: self
                .session
                .tracks
                .get(index)
                .map(|t| t.name.clone())
                .unwrap_or_default(),
            max_depth: track.max_depth,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
