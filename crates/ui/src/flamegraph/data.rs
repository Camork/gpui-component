//! Data model and tree-to-flat layout for multi-track flame graphs.

use std::time::Duration;

use gpui::{Hsla, hsla};

/// A single flame frame.
pub struct FlameFrame {
    pub name: String,
    /// Absolute offset from the start of the owning track.
    pub start: Duration,
    pub duration: Duration,
    pub children: Vec<FlameFrame>,
}

impl FlameFrame {
    pub fn new(name: impl Into<String>, start: Duration, duration: Duration) -> Self {
        Self {
            name: name.into(),
            start,
            duration,
            children: Vec::new(),
        }
    }

    pub fn with_children(mut self, children: Vec<FlameFrame>) -> Self {
        self.children = children;
        self
    }
}

/// A track shares one time axis with every other track.
pub struct Track {
    pub name: String,
    pub collapsed: bool,
    pub roots: Vec<FlameFrame>,
}

pub struct Session {
    pub tracks: Vec<Track>,
}

/// One frame after flattening: absolute times in seconds, plus its row
/// (nesting depth) and its deterministic color.
pub struct FlatFrame {
    pub name: String,
    pub color: Hsla,
    pub row: usize,
    pub abs_start: f64,
    pub abs_end: f64,
}

pub struct TrackLayout {
    pub name: String,
    /// Highest nesting depth + this track draws rows 0..=max_depth.
    pub max_depth: usize,
    pub frames: Vec<FlatFrame>,
}

/// The flattened view of a session, recomputed whenever the data changes.
pub struct FlatSession {
    pub tracks: Vec<TrackLayout>,
    /// End of the whole trace (max frame end over all tracks), seconds.
    pub session_end: f64,
}

pub fn secs(d: &Duration) -> f64 {
    d.as_secs_f64()
}

fn layout_track(track: &Track) -> TrackLayout {
    let mut frames = Vec::new();
    let mut max_depth = 0usize;
    fn visit(node: &FlameFrame, depth: usize, frames: &mut Vec<FlatFrame>, max_depth: &mut usize) {
        *max_depth = (*max_depth).max(depth);
        let start = secs(&node.start);
        frames.push(FlatFrame {
            name: node.name.clone(),
            color: color_for(&node.name),
            row: depth,
            abs_start: start,
            abs_end: start + secs(&node.duration),
        });
        for child in &node.children {
            visit(child, depth + 1, frames, max_depth);
        }
    }
    for root in &track.roots {
        visit(root, 0, &mut frames, &mut max_depth);
    }
    TrackLayout {
        name: track.name.clone(),
        max_depth,
        frames,
    }
}

/// Flatten a session into per-track flat frame lists.
///
/// Children of the same parent are placed on the row below it; frames on the
/// same row never overlap in time (the fake-data generator guarantees that),
/// so row + time hit-testing is unambiguous.
pub fn flatten(session: &Session) -> FlatSession {
    let mut tracks = Vec::new();
    let mut session_end = 0.0f64;
    for (_i, track) in session.tracks.iter().enumerate() {
        let layout = layout_track(track);
        if let Some(end) = layout
            .frames
            .iter()
            .map(|f| f.abs_end)
            .max_by(f64::total_cmp)
        {
            session_end = session_end.max(end);
        }
        tracks.push(layout);
    }
    FlatSession {
        tracks,
        session_end,
    }
}

fn fnv1a(name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Deterministic hue for a function name: the same name always gets the same
/// color wherever it appears.
pub fn color_for(name: &str) -> Hsla {
    let h = fnv1a(name);
    // FNV hash is a full 32-bit distribution; fold it into hue and spread with
    // the golden angle so nearby names do not land on nearby hues.
    let hue = (((h % 997) as f32) / 997.0 + 0.618_033_9).fract();
    let sat = 0.42 + ((h >> 16) % 16) as f32 / 100.0;
    let light = 0.52 + ((h >> 8) % 14) as f32 / 100.0;
    hsla(hue, sat, light, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: f64) -> Duration {
        Duration::from_secs_f64(v / 1000.0)
    }

    fn session() -> Session {
        let root = FlameFrame::new("a", ms(0.0), ms(100.0)).with_children(vec![
            FlameFrame::new("b", ms(10.0), ms(40.0)).with_children(vec![FlameFrame::new(
                "c",
                ms(15.0),
                ms(20.0),
            )]),
            FlameFrame::new("d", ms(55.0), ms(30.0)),
        ]);
        let root2 = FlameFrame::new("e", ms(120.0), ms(50.0));
        Session {
            tracks: vec![Track {
                name: "t0".into(),
                collapsed: false,
                roots: vec![root, root2],
            }],
        }
    }

    #[test]
    fn flatten_assigns_rows_and_absolute_times() {
        let flat = flatten(&session());
        assert!((flat.session_end - 0.17).abs() < 1e-9);
        assert_eq!(flat.tracks.len(), 1);
        let t0 = &flat.tracks[0];
        assert_eq!(t0.max_depth, 2);
        let by_name = |name: &str| {
            t0.frames
                .iter()
                .find(|f| f.name == name)
                .expect("frame present")
        };
        assert_eq!(by_name("a").row, 0);
        assert_eq!(by_name("a").abs_start, 0.0);
        assert_eq!(by_name("a").abs_end, 0.1);
        assert_eq!(by_name("b").row, 1);
        assert_eq!(by_name("b").abs_start, 0.01);
        assert_eq!(by_name("c").row, 2);
        assert_eq!(by_name("c").abs_start, 0.015);
        assert_eq!(by_name("d").row, 1);
        assert_eq!(by_name("e").row, 0);
        assert_eq!(by_name("e").abs_start, 0.12);
    }

    #[test]
    fn colors_are_deterministic() {
        assert_eq!(color_for("render"), color_for("render"));
        assert_eq!(color_for("compute"), color_for("compute"));
        assert_ne!(color_for("render"), color_for("compute"));
    }
}
