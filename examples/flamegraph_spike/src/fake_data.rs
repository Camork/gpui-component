//! Fake-data generators for the flame graph spike.
//!
//! Two presets:
//! - **Demo**: a handful of tracks with dozens of frames, easy to eyeball.
//! - **Pressure**: thousands of frames (deep and wide trees) to smoke-test the
//!   render pipeline's throughput before real trace loading exists.
//!
//! A deterministic RNG keeps the output stable between runs, so visual bugs
//! reproduce. Every child is placed strictly inside its parent's time span and
//! siblings on the same row never overlap in time, which the hit-testing relies
//! on.

use std::time::Duration;

use gpui_component::flamegraph::{FlameFrame, Session, Track};

#[derive(Clone, PartialEq, Debug)]
pub enum Preset {
    Demo,
    Pressure,
    Mega,
    Sparse,
}

/// Small deterministic xorshift-style generator (SplitMix64).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform float in `[0, 1)`.
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform integer in `[lo, hi]` inclusive.
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() % (hi - lo + 1) as u64) as usize
    }
}

pub fn fake_session(preset: Preset) -> Session {
    match preset {
        Preset::Demo => demo(),
        Preset::Pressure => pressure(),
        Preset::Mega => mega(),
        Preset::Sparse => sparse(),
    }
}

// ── name pools ──────────────────────────────────────────────────────────────

const RENDER_FNS: &[&str] = &[
    "Renderer::present",
    "CmdBuffer::record",
    "Texture::upload",
    "Pipeline::bind",
    "Framebuffer::clear",
    "Mesh::draw",
    "Shader::compile",
    "Tessellate::run",
    "GpuQueue::submit",
    "Swapchain::acquire",
];

const COMPUTE_FNS: &[&str] = &[
    "Compute::dispatch",
    "SimulateParticles",
    "Physics::step",
    "Integrate::run",
    "Broadphase::cull",
    "Narrowphase::collide",
    "SolveConstraints",
    "SpatialHash::build",
    "Particles::update",
    "ForceField::eval",
];

const IO_FNS: &[&str] = &[
    "Io::read",
    "AssetStore::load",
    "Decompress::run",
    "DecodeImage",
    "ParseMesh::load",
    "AsyncIo::poll",
    "File::map",
    "AssetCache::evict",
    "Stream::parse",
    "Vfs::stat",
];

const AUDIO_FNS: &[&str] = &[
    "Audio::mix",
    "Resample::run",
    "Dsp::filter",
    "Clip::decode",
    "Mixer::render",
    "Envelope::tick",
    "Compressor::process",
    "Buffer::flush",
];

const PRESSURE_FNS: &[&str] = &[
    "Frame::render",
    "Scene::update",
    "Physics::step",
    "Collide::narrowphase",
    "Collide::broadphase",
    "Mesh::cull",
    "Mesh::draw",
    "Light::compute",
    "Shadow::render",
    "PostFx::bloom",
    "PostFx::tonemap",
    "Particles::sim",
    "Particles::sort",
    "Ui::layout",
    "Ui::paint",
    "Asset::stream",
    "Asset::decode",
    "Network::recv",
    "Network::poll",
    "Lua::call",
    "Lua::gc",
    "Script::await",
    "Job::dispatch",
    "Job::join",
];

// ── demo preset ─────────────────────────────────────────────────────────────

fn demo() -> Session {
    let mut rng = Rng::new(0xd3_000_000);
    let mut render = Track {
        name: "thread-render".into(),
        collapsed: false,
        roots: Vec::new(),
    };
    let mut compute = Track {
        name: "thread-compute".into(),
        collapsed: false,
        roots: Vec::new(),
    };
    let mut io = Track {
        name: "thread-io".into(),
        collapsed: false,
        roots: Vec::new(),
    };

    let mut t = 0.0; // seconds
    for i in 0..6 {
        let dur = 0.06 + rng.f() * 0.06;
        let name = RENDER_FNS[rng.range(0, RENDER_FNS.len() - 1)];
        render
            .roots
            .push(demo_call(name, t, dur, 3, RENDER_FNS, &mut rng, i));
        t += dur + 0.01 + rng.f() * 0.03;
    }

    let mut t = 0.02;
    for i in 0..5 {
        let dur = 0.05 + rng.f() * 0.08;
        let name = COMPUTE_FNS[rng.range(0, COMPUTE_FNS.len() - 1)];
        compute
            .roots
            .push(demo_call(name, t, dur, 4, COMPUTE_FNS, &mut rng, i));
        t += dur + 0.02 + rng.f() * 0.04;
    }

    let mut t = 0.0;
    for i in 0..3 {
        let dur = 0.03 + rng.f() * 0.03;
        let name = IO_FNS[rng.range(0, IO_FNS.len() - 1)];
        io.roots
            .push(demo_call(name, t, dur, 2, IO_FNS, &mut rng, i));
        t += dur + 0.04 + rng.f() * 0.1;
    }

    let mut audio = Track {
        name: "thread-audio".into(),
        collapsed: false,
        roots: Vec::new(),
    };
    let mut t = 0.03;
    for i in 0..4 {
        let dur = 0.008 + rng.f() * 0.008;
        let name = AUDIO_FNS[rng.range(0, AUDIO_FNS.len() - 1)];
        audio
            .roots
            .push(demo_call(name, t, dur, 2, AUDIO_FNS, &mut rng, i));
        t += dur + 0.006 + rng.f() * 0.02;
    }

    let mut w1 = Track {
        name: "thread-worker-1".into(),
        collapsed: false,
        roots: Vec::new(),
    };
    let mut t = 0.01;
    for i in 0..5 {
        let dur = 0.04 + rng.f() * 0.06;
        let name = COMPUTE_FNS[rng.range(0, COMPUTE_FNS.len() - 1)];
        w1.roots
            .push(demo_call(name, t, dur, 4, COMPUTE_FNS, &mut rng, i));
        t += dur + 0.02 + rng.f() * 0.03;
    }

    let mut w2 = Track {
        name: "thread-worker-2".into(),
        collapsed: false,
        roots: Vec::new(),
    };
    let mut t = 0.04;
    for i in 0..4 {
        let dur = 0.05 + rng.f() * 0.07;
        let name = RENDER_FNS[rng.range(0, RENDER_FNS.len() - 1)];
        w2.roots
            .push(demo_call(name, t, dur, 3, RENDER_FNS, &mut rng, i));
        t += dur + 0.01 + rng.f() * 0.04;
    }

    let mut w3 = Track {
        name: "thread-worker-3".into(),
        collapsed: false,
        roots: Vec::new(),
    };
    let mut t = 0.02;
    for i in 0..6 {
        let dur = 0.03 + rng.f() * 0.05;
        let name = IO_FNS[rng.range(0, IO_FNS.len() - 1)];
        w3.roots
            .push(demo_call(name, t, dur, 3, IO_FNS, &mut rng, i));
        t += dur + 0.02 + rng.f() * 0.03;
    }

    Session {
        tracks: vec![render, compute, io, audio, w1, w2, w3],
    }
}

/// Build one demo call at absolute `start` with `dur`, splitting its span into
/// `depth` more levels of children. Children get absolute offsets inside
/// `[start, start + dur]` and never overlap.
fn demo_call(
    name: &str,
    start: f64,
    dur: f64,
    depth: usize,
    pool: &[&str],
    rng: &mut Rng,
    salt: usize,
) -> FlameFrame {
    let mut children = Vec::new();
    if depth > 0 && dur > 0.004 {
        let n = rng.range(2, 4);
        let gap = dur * 0.05;
        let w = (dur - gap * (n as f64 + 1.0)) / n as f64;
        let mut t = start + gap;
        for k in 0..n {
            let child_name = pool[(rng.next() as usize + salt * 31 + k * 7) % pool.len()];
            let child_depth = if rng.f() < 0.7 { depth - 1 } else { 0 };
            children.push(demo_call(
                child_name,
                t,
                w,
                child_depth,
                pool,
                rng,
                salt + k,
            ));
            t += w + gap;
        }
    }
    FlameFrame::new(
        name,
        Duration::from_secs_f64(start),
        Duration::from_secs_f64(dur),
    )
    .with_children(children)
}

// ── pressure preset ─────────────────────────────────────────────────────────

/// Thousands of frames across a few tracks: deep call chains and wide fan-outs
/// at the same time, filling a ~12s trace.
fn pressure() -> Session {
    const TRACK_COUNT: usize = 4;
    const ROOTS_PER_TRACK: usize = 28;
    const MAX_DEPTH: usize = 30;
    const TRACK_STRIDE: f64 = 0.09; // stagger start so tracks interleave

    let mut rng = Rng::new(0xfeed_beef);
    let names = ["render", "compute", "io", "gpu"];
    let mut tracks = Vec::new();
    for ti in 0..TRACK_COUNT {
        let mut track = Track {
            name: format!("thread-{}", names[ti]),
            collapsed: false,
            roots: Vec::new(),
        };
        let mut t = TRACK_STRIDE * ti as f64;
        for i in 0..ROOTS_PER_TRACK {
            let dur = 0.2 + rng.f() * 0.35;
            let depth = rng.range(20, MAX_DEPTH);
            let name = PRESSURE_FNS[rng.range(0, PRESSURE_FNS.len() - 1)];
            track
                .roots
                .push(pressure_call(name, t, dur, depth, &mut rng, ti * 1000 + i));
            t += dur + 0.03 + rng.f() * 0.06;
        }
        tracks.push(track);
    }
    Session { tracks }
}

/// Build one pressure call at absolute `start` with `dur`. The *first* child
/// keeps most of the span and recurses with `depth - 1` unconditionally, so
/// every root carries a chain that reaches exactly `depth` frames of nesting
/// (the flame chart stays deep even when siblings thin out). Extra siblings
/// fill the remaining span as *leaf* frames, so the trace is wide without
/// exponential blow-up.
fn pressure_call(
    name: &str,
    start: f64,
    dur: f64,
    depth: usize,
    rng: &mut Rng,
    salt: usize,
) -> FlameFrame {
    let mut children = Vec::new();
    if depth > 0 {
        let gap = dur * 0.02;
        let main_w = dur * 0.8;
        let child_name = PRESSURE_FNS[(rng.next() as usize + salt) % PRESSURE_FNS.len()];
        children.push(pressure_call(
            child_name,
            start + gap,
            main_w,
            depth - 1,
            rng,
            salt + 1,
        ));

        // 1-3 leaf siblings share the leftover span after the main child
        let extras = rng.range(1, 4);
        let w = (dur - main_w - gap * (extras as f64 + 2.0)) / extras as f64;
        let mut t = start + gap + main_w + gap;
        for k in 0..extras {
            let child_name =
                PRESSURE_FNS[(rng.next() as usize + salt * 13 + k * 29) % PRESSURE_FNS.len()];
            children.push(FlameFrame::new(
                child_name,
                Duration::from_secs_f64(t),
                Duration::from_secs_f64(w),
            ));
            t += w + gap;
        }
    }
    FlameFrame::new(
        name,
        Duration::from_secs_f64(start),
        Duration::from_secs_f64(dur),
    )
    .with_children(children)
}

// ── mega preset (1,000,000+ frames) ──────────────────────────────────────────

fn mega() -> Session {
    const TRACK_COUNT: usize = 6;
    const ROOTS_PER_TRACK: usize = 12;
    let names = ["render", "compute", "io", "gpu", "worker-1", "worker-2"];
    let mut rng = Rng::new(0x7777_8888);
    let mut tracks = Vec::new();

    for ti in 0..TRACK_COUNT {
        let mut track = Track {
            name: format!("thread-{}", names[ti]),
            collapsed: false,
            roots: Vec::new(),
        };
        let mut t = 0.05 * ti as f64;
        for i in 0..ROOTS_PER_TRACK {
            let dur = 0.5 + rng.f() * 0.5;
            let name = PRESSURE_FNS[rng.range(0, PRESSURE_FNS.len() - 1)];
            track.roots.push(mega_call(name, t, dur, 7, &mut rng, ti * 100 + i));
            t += dur + 0.05;
        }
        tracks.push(track);
    }
    Session { tracks }
}

fn mega_call(
    name: &str,
    start: f64,
    dur: f64,
    depth: usize,
    rng: &mut Rng,
    salt: usize,
) -> FlameFrame {
    let mut children = Vec::new();
    if depth > 0 && dur > 0.000001 {
        let fanout = rng.range(3, 5);
        let gap = dur * 0.01;
        let w = (dur - gap * (fanout as f64 + 1.0)) / fanout as f64;
        let mut t = start + gap;
        for k in 0..fanout {
            let child_name = PRESSURE_FNS[(rng.next() as usize + salt * 13 + k * 29) % PRESSURE_FNS.len()];
            children.push(mega_call(child_name, t, w, depth - 1, rng, salt + k + 1));
            t += w + gap;
        }
    }
    FlameFrame::new(
        name,
        Duration::from_secs_f64(start),
        Duration::from_secs_f64(dur),
    )
    .with_children(children)
}

// ── sparse preset (sparse micro-bursts across a 10s timeline) ─────────────────

fn sparse() -> Session {
    const TRACK_COUNT: usize = 4;
    let names = ["ui_thread", "worker_thread", "io_thread", "timer_thread"];
    let mut tracks = Vec::new();

    for ti in 0..TRACK_COUNT {
        let mut track = Track {
            name: names[ti].into(),
            collapsed: false,
            roots: Vec::new(),
        };

        match ti {
            0 => {
                // UI Thread: 3 isolated micro-bursts at 1.0s, 5.0s, 9.0s
                track.roots.push(
                    FlameFrame::new("event_loop_tick", Duration::from_secs_f64(1.0), Duration::from_secs_f64(0.005))
                        .with_children(vec![
                            FlameFrame::new("process_input", Duration::from_secs_f64(1.001), Duration::from_secs_f64(0.002))
                        ])
                );
                track.roots.push(
                    FlameFrame::new("event_loop_tick", Duration::from_secs_f64(5.0), Duration::from_secs_f64(0.008))
                        .with_children(vec![
                            FlameFrame::new("relayout", Duration::from_secs_f64(5.001), Duration::from_secs_f64(0.004))
                                .with_children(vec![
                                    FlameFrame::new("flex_node", Duration::from_secs_f64(5.002), Duration::from_secs_f64(0.001)),
                                    FlameFrame::new("paint_rect", Duration::from_secs_f64(5.0035), Duration::from_secs_f64(0.001)),
                                ])
                        ])
                );
                track.roots.push(
                    FlameFrame::new("event_loop_tick", Duration::from_secs_f64(9.0), Duration::from_secs_f64(0.003))
                );
            }
            1 => {
                // Worker Thread: a single 20ms burst at 3.2s
                track.roots.push(
                    FlameFrame::new("async_task", Duration::from_secs_f64(3.2), Duration::from_secs_f64(0.020))
                        .with_children(vec![
                            FlameFrame::new("parse_json", Duration::from_secs_f64(3.202), Duration::from_secs_f64(0.008)),
                            FlameFrame::new("validate_schema", Duration::from_secs_f64(3.212), Duration::from_secs_f64(0.005)),
                        ])
                );
            }
            2 => {
                // IO Thread: 2 tiny disk reads at 0.5s and 7.8s
                track.roots.push(
                    FlameFrame::new("read_file", Duration::from_secs_f64(0.5), Duration::from_secs_f64(0.001))
                );
                track.roots.push(
                    FlameFrame::new("write_cache", Duration::from_secs_f64(7.8), Duration::from_secs_f64(0.002))
                );
            }
            3 => {
                // Timer Thread: periodic 1ms ticks every 2 seconds across 10s
                for t_sec in [2.0, 4.0, 6.0, 8.0] {
                    track.roots.push(
                        FlameFrame::new("timer_callback", Duration::from_secs_f64(t_sec), Duration::from_secs_f64(0.001))
                    );
                }
            }
            _ => {}
        }

        tracks.push(track);
    }

    Session { tracks }
}

/// Total number of frames a session expands to (used by the header status).
pub fn count_frames(session: &Session) -> usize {
    fn count(node: &FlameFrame) -> usize {
        1 + node.children.iter().map(count).sum::<usize>()
    }
    session
        .tracks
        .iter()
        .map(|t| t.roots.iter().map(count).sum::<usize>())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::flamegraph::flatten;

    #[test]
    fn demo_preset_is_small_but_structured() {
        let s = fake_session(Preset::Demo);
        let n = count_frames(&s);
        assert!(n >= 40 && n <= 600, "demo should be eyeballable, got {n}");
        let flat = flatten(&s);
        assert!(flat.session_end > 0.4 && flat.session_end < 2.0);
        assert!(flat.tracks.len() >= 4);
        assert!(flat.tracks.iter().any(|t| t.max_depth >= 2));
    }

    #[test]
    fn pressure_preset_has_thousands_of_frames() {
        let s = fake_session(Preset::Pressure);
        let n = count_frames(&s);
        assert!(n >= 5_000 && n <= 30_000, "pressure preset: got {n}");
        let flat = flatten(&s);
        assert!(flat.tracks.len() >= 4);
        assert!(
            flat.tracks.iter().any(|t| t.max_depth >= 20),
            "pressure should be deep"
        );
    }

    #[test]
    fn children_never_escape_their_parent() {
        for preset in [Preset::Demo, Preset::Pressure] {
            let s = fake_session(preset);
            for track in &s.tracks {
                fn check(node: &FlameFrame) {
                    for child in &node.children {
                        let c_start = child.start.as_secs_f64();
                        let c_end = c_start + child.duration.as_secs_f64();
                        let p_end = node.start.as_secs_f64() + node.duration.as_secs_f64();
                        assert!(
                            c_start >= node.start.as_secs_f64() - 1e-12,
                            "{} child {c_start} < parent",
                            node.name
                        );
                        assert!(
                            c_end <= p_end + 1e-12,
                            "{} child end {c_end} > parent end {p_end}",
                            node.name
                        );
                        check(child);
                    }
                }
                for root in &track.roots {
                    check(root);
                }
            }
        }
    }

    #[test]
    fn siblings_on_same_row_do_not_overlap() {
        for preset in [Preset::Demo, Preset::Pressure] {
            let s = fake_session(preset);
            let flat = flatten(&s);
            for track in &flat.tracks {
                for row in 0..=track.max_depth {
                    let mut spans: Vec<(f64, f64)> = track.rows[row]
                        .iter()
                        .map(|f| (f.abs_start, f.abs_end))
                        .collect();
                    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
                    for w in spans.windows(2) {
                        assert!(w[1].0 >= w[0].1 - 1e-9, "overlap on row {row}: {:?}", w);
                    }
                }
            }
        }
    }
}
