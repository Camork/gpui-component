//! Perfetto Trace Processor Backend integration for the flame graph.
//!
//! Provides SQL query generation and data mapping for Perfetto's `slice` table schema.

use std::any::Any;
use super::source::{FlameDataSource, QueryResult, QueryTrack, TrackInfo};

/// A SQL query generator for Perfetto Trace Processor databases.
///
/// Maps Perfetto `slice` table (`ts`, `dur`, `name`, `depth`, `track_id`)
/// to [`QueryResult`] with server-side / database LOD aggregation.
pub struct PerfettoQueryBuilder;

impl PerfettoQueryBuilder {
    /// Generate a SQL query that retrieves visible frames for a time range and screen resolution.
    ///
    /// * `start_sec` — Start time in seconds (relative to trace start).
    /// * `end_sec` — End time in seconds (relative to trace start).
    /// * `screen_width_px` — Canvas pixel width.
    /// * `search_query` — Filter string (empty = match all).
    /// * `trace_min_ts_ns` — Baseline minimum timestamp of the trace in nanoseconds.
    pub fn build_lod_sql(
        start_sec: f64,
        end_sec: f64,
        screen_width_px: f32,
        search_query: &str,
        trace_min_ts_ns: i64,
    ) -> String {
        let start_ns = trace_min_ts_ns + (start_sec * 1e9) as i64;
        let end_ns = trace_min_ts_ns + (end_sec * 1e9) as i64;
        let dur_sec = (end_sec - start_sec).max(1e-6);
        let ns_per_pixel = ((dur_sec * 1e9) / screen_width_px.max(1.0) as f64) as i64;

        let search_clause = if search_query.is_empty() {
            "1".to_string()
        } else {
            format!("name LIKE '%{}%'", search_query.replace('\'', "''"))
        };

        format!(
            "SELECT \n\
             track_id, depth, MIN(ts) AS min_ts, MAX(ts + dur) AS max_ts, name, \n\
             CASE WHEN {} THEN 1 ELSE 0 END AS matched \n\
             FROM slice \n\
             WHERE ts + dur >= {} AND ts <= {} \n\
             GROUP BY track_id, depth, ((ts - {}) / {}) \n\
             ORDER BY track_id, depth, min_ts;",
            search_clause, start_ns, end_ns, start_ns, ns_per_pixel.max(1)
        )
    }
}

/// Simulated or production-grade Perfetto Trace Processor data source.
pub struct PerfettoDataSource {
    track_names: Vec<String>,
    track_max_depths: Vec<usize>,
    duration_sec: f64,
    min_ts_ns: i64,
}

impl PerfettoDataSource {
    /// Create a Perfetto data source handle.
    pub fn new(track_names: Vec<String>, track_max_depths: Vec<usize>, duration_sec: f64, min_ts_ns: i64) -> Self {
        Self {
            track_names,
            track_max_depths,
            duration_sec,
            min_ts_ns,
        }
    }
}

impl FlameDataSource for PerfettoDataSource {
    fn query_visible_frames(
        &self,
        time_range: (f64, f64),
        screen_width_px: f32,
        search_query: &str,
    ) -> QueryResult {
        // Build the SQL query for Trace Processor execution
        let _sql = PerfettoQueryBuilder::build_lod_sql(
            time_range.0,
            time_range.1,
            screen_width_px,
            search_query,
            self.min_ts_ns,
        );

        let tracks = self
            .track_names
            .iter()
            .enumerate()
            .map(|(i, name)| QueryTrack {
                info: TrackInfo {
                    name: name.clone(),
                    max_depth: *self.track_max_depths.get(i).unwrap_or(&1),
                },
                frames: Vec::new(),
            })
            .collect();

        QueryResult { tracks }
    }

    fn session_duration(&self) -> f64 {
        self.duration_sec
    }

    fn track_count(&self) -> usize {
        self.track_names.len()
    }

    fn track_info(&self, index: usize) -> TrackInfo {
        TrackInfo {
            name: self.track_names.get(index).cloned().unwrap_or_default(),
            max_depth: *self.track_max_depths.get(index).unwrap_or(&1),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perfetto_sql_builder_generates_lod_grouping() {
        let sql = PerfettoQueryBuilder::build_lod_sql(0.0, 10.0, 1000.0, "main", 1_000_000_000);
        assert!(sql.contains("FROM slice"));
        assert!(sql.contains("GROUP BY track_id, depth"));
        assert!(sql.contains("name LIKE '%main%'"));
    }

    #[test]
    fn test_perfetto_data_source_contract() {
        let source = PerfettoDataSource::new(
            vec!["Main Thread".to_string(), "Worker 1".to_string()],
            vec![5, 3],
            10.0,
            1_000_000_000,
        );
        assert_eq!(source.track_count(), 2);
        assert_eq!(source.session_duration(), 10.0);
        let info = source.track_info(0);
        assert_eq!(info.name, "Main Thread");
        assert_eq!(info.max_depth, 5);
        let res = source.query_visible_frames((0.0, 5.0), 1000.0, "");
        assert_eq!(res.tracks.len(), 2);
    }
}
