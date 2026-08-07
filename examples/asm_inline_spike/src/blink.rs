//! Blink cursor, modeled on `crates/ui/src/input/blink_cursor.rs`: an entity
//! that toggles `visible` every 500ms while unpaused; pausing keeps it on for
//! 300ms after a keystroke so the caret stays solid. The view observes it to
//! repaint on each toggle.

use std::time::Duration;

use gpui::{Context, Entity, Task};

const BLINK_INTERVAL: Duration = Duration::from_millis(500);
const BLINK_PAUSE_DELAY: Duration = Duration::from_millis(300);

#[cfg(not(target_os = "macos"))]
pub(crate) const CURSOR_WIDTH: f32 = 2.0;
#[cfg(target_os = "macos")]
pub(crate) const CURSOR_WIDTH: f32 = 1.5;

pub(crate) struct BlinkCursor {
    visible: bool,
    paused: bool,
    epoch: usize,
    _task: Task<()>,
}

impl BlinkCursor {
    pub(crate) fn new() -> Self {
        Self {
            visible: false,
            paused: false,
            epoch: 0,
            _task: Task::ready(()),
        }
    }

    pub(crate) fn start(&mut self, cx: &mut Context<Self>) {
        self.blink(self.epoch, cx);
    }

    fn next_epoch(&mut self) -> usize {
        self.epoch += 1;
        self.epoch
    }

    fn blink(&mut self, epoch: usize, cx: &mut Context<Self>) {
        if self.paused || epoch != self.epoch {
            self.visible = true;
            return;
        }

        self.visible = !self.visible;
        cx.notify();

        let epoch = self.next_epoch();
        self._task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BLINK_INTERVAL).await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| this.blink(epoch, cx));
            }
        });
    }

    pub(crate) fn visible(&self) -> bool {
        self.paused || self.visible
    }

    pub(crate) fn pause(&mut self, cx: &mut Context<Self>) {
        self.paused = true;
        self.visible = true;
        cx.notify();

        let epoch = self.next_epoch();
        self._task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BLINK_PAUSE_DELAY).await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    this.paused = false;
                    this.blink(epoch, cx);
                });
            }
        });
    }
}

/// Type alias to keep view code terse.
pub(crate) type BlinkEntity = Entity<BlinkCursor>;
