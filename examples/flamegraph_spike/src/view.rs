//! `AppView`: Example application container wrapping the [`FlameGraph`] component.
//!
//! Demonstrates how external applications use `FlameGraphState` and `FlameGraph`
//! while building their own custom toolbar controls (search input, presets, stats).

use gpui::{
    Context, Entity, IntoElement, Render, Subscription, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    flamegraph::{FlameGraph, FlameGraphState, format_duration},
    input::{Input, InputEvent, InputState},
};

use crate::fake_data::{Preset, count_frames, fake_session};

pub(crate) struct AppView {
    pub(crate) state: Entity<FlameGraphState>,
    pub(crate) preset: Preset,
    pub(crate) input_state: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let preset = Preset::Demo;
        let session = fake_session(preset.clone());
        let state = cx.new(|_| FlameGraphState::new(session));

        let input_state = cx.new(|cx| InputState::new(window, cx).placeholder("filter frames…"));
        let _subscriptions = vec![cx.subscribe_in(&input_state, window, {
            let input_state = input_state.clone();
            let state = state.clone();
            move |_this: &mut Self, _, ev: &InputEvent, _window, cx| match ev {
                InputEvent::Change => {
                    let value = input_state.read(cx).value().to_string();
                    state.update(cx, |flame_state, cx| {
                        flame_state.set_filter(&value, cx);
                    });
                }
                _ => {}
            }
        })];

        Self {
            state,
            preset,
            input_state,
            _subscriptions,
        }
    }

    pub(crate) fn switch_preset(&mut self, preset: Preset, cx: &mut Context<Self>) {
        let session = fake_session(preset.clone());
        self.preset = preset;
        self.state.update(cx, |flame_state, cx| {
            flame_state.set_session(session, cx);
        });
        cx.notify();
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let flame = self.state.read(cx);
        let session = flame.session();
        let flat = flame.flat();
        let vp = flame.viewport();
        let total = count_frames(session);
        let max_depth = flat
            .tracks
            .iter()
            .map(|t| t.max_depth)
            .max()
            .unwrap_or(0);
        let demo_active = matches!(self.preset, Preset::Demo);
        let time_format = flame.time_format();

        div()
            .h(px(44.0))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .bg(cx.theme().background)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("flamegraph demo"),
            )
            .child(
                Button::new("preset-demo")
                    .ghost()
                    .toggled(demo_active)
                    .label("Demo")
                    .on_click(cx.listener(
                        |this: &mut Self, _: &gpui::ClickEvent, _: &mut Window, cx| {
                            this.switch_preset(Preset::Demo, cx);
                        },
                    )),
            )
            .child(
                Button::new("preset-pressure")
                    .ghost()
                    .toggled(!demo_active)
                    .label("Pressure")
                    .on_click(cx.listener(
                        |this: &mut Self, _: &gpui::ClickEvent, _: &mut Window, cx| {
                            this.switch_preset(Preset::Pressure, cx);
                        },
                    )),
            )
            .child(Input::new(&self.input_state).w(px(220.0)))
            .child(div().flex_grow_1())
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{total} frames · depth {max_depth}")),
            )
            .child(div().text_color(cx.theme().muted_foreground).child(format!(
                "{} → {}",
                format_duration(vp.start, time_format),
                format_duration(vp.end, time_format)
            )))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("wheel=pan · shift+wheel=v-scroll · ctrl+wheel=zoom · drag=zoom · dblclick=reset"),
            )
    }
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .child(self.toolbar(cx))
            .child(div().flex_grow_1().child(FlameGraph::new(&self.state)))
    }
}
