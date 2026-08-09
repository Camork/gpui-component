//! Time formatting utilities for flame graph labels and rulers.

/// Time unit ladder, smallest to largest. `Ns` is the hard floor (nothing is
/// ever rendered below nanoseconds); `Min` the top of the ladder, reached only
/// when the zoomed-out window (or a tooltip value) actually gets that large.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeUnit {
    Ns,
    Us,
    Ms,
    S,
    Min,
}

impl TimeUnit {
    /// One unit, in seconds.
    pub(crate) fn seconds(self) -> f64 {
        match self {
            TimeUnit::Ns => 1e-9,
            TimeUnit::Us => 1e-6,
            TimeUnit::Ms => 1e-3,
            TimeUnit::S => 1.0,
            TimeUnit::Min => 60.0,
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            TimeUnit::Ns => "ns",
            TimeUnit::Us => "µs",
            TimeUnit::Ms => "ms",
            TimeUnit::S => "s",
            TimeUnit::Min => "min",
        }
    }
}

/// Formatting knobs for time labels. `decimals` is fixed per config (2 by
/// default): a value/step is shown in the *largest* unit where it still fits
/// within that many decimal places, and never below [`TimeUnit::Ns`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeFormatConfig {
    pub decimals: u8,
    pub min_unit: TimeUnit,
}

impl Default for TimeFormatConfig {
    fn default() -> Self {
        Self {
            decimals: 2,
            min_unit: TimeUnit::Ns,
        }
    }
}

/// Choose the unit to display `scale` (a value or a tick step, seconds) in:
/// - a value stays in the *largest* unit that still fits within the configured
///   decimals (so `0.01` shows as `0.01 s`, not `10.00 ms`), keeping the
///   ladder `µs → ms → s` as the scale grows;
/// - `min` only appears once the scale actually reaches a whole minute, so a
///   `1.50 s` duration never reads as `0.03 min`;
/// - falls back to [`TimeUnit::Ns`] (the hard floor) below every unit's
///   precision.
fn unit_for_scale(scale: f64, config: &TimeFormatConfig) -> TimeUnit {
    if scale >= 60.0 {
        return TimeUnit::Min;
    }
    let floor = 10f64.powi(-(config.decimals as i32));
    for unit in [TimeUnit::S, TimeUnit::Ms, TimeUnit::Us] {
        if scale >= floor * unit.seconds() {
            return unit;
        }
    }
    config.min_unit
}

fn format_in_unit(secs: f64, unit: TimeUnit, config: &TimeFormatConfig) -> String {
    let value = secs / unit.seconds();
    // At the configured floor (`min_unit`) the unit itself is the smallest
    // quantum: decimals would print *fractions of the smallest unit* (e.g.
    // `0.01 ns`), which is below the resolution floor. Derived units keep the
    // configured decimals (a fractional `µs`/`ms`/s is still above the floor).
    let decimals = if unit == config.min_unit {
        0
    } else {
        config.decimals
    };
    format!("{:.*} {}", decimals as usize, value, unit.suffix())
}

/// Value-based duration formatting (tooltips, toolbar range): the unit follows
/// the value's own magnitude, so a duration reads in its most natural unit.
pub fn format_duration(secs: f64, config: &TimeFormatConfig) -> String {
    format_in_unit(secs, unit_for_scale(secs, config), config)
}

/// Step-based tick formatting (ruler): the unit follows the tick *step*, so
/// every label on the ruler shares one unit that grows as the user zooms out
/// (`ns → µs → ms → s → min`) without mixing units on a single ruler.
pub fn format_tick(secs: f64, step_secs: f64, config: &TimeFormatConfig) -> String {
    format_in_unit(secs, unit_for_scale(step_secs, config), config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_uses_value_based_units() {
        assert_eq!(
            format_duration(90.0, &TimeFormatConfig::default()),
            "1.50 min"
        );
        assert_eq!(format_duration(1.5, &TimeFormatConfig::default()), "1.50 s");
        // still fits in seconds within 2 decimals → seconds ("0.01 s" floor)
        assert_eq!(
            format_duration(0.012, &TimeFormatConfig::default()),
            "0.01 s"
        );
        // one notch smaller and it no longer fits → millisecond unit
        assert_eq!(
            format_duration(0.005, &TimeFormatConfig::default()),
            "5.00 ms"
        );
        assert_eq!(
            format_duration(0.000003, &TimeFormatConfig::default()),
            "3.00 µs"
        );
        // ns is the hard floor: below-nanosecond values collapse to integer ns
        // (never "0.01 ns" — that would be a sub-floor fraction).
        assert_eq!(format_duration(1e-12, &TimeFormatConfig::default()), "0 ns");
        assert_eq!(
            format_duration(1.4e-9, &TimeFormatConfig::default()),
            "1 ns"
        );
    }

    #[test]
    fn format_tick_switches_unit_with_step() {
        let c = TimeFormatConfig::default();
        // step 0.01 s → still seconds ("0.01 s"), one notch smaller → ms ("5.00 ms")
        assert_eq!(format_tick(0.03, 0.01, &c), "0.03 s");
        assert_eq!(format_tick(0.015, 0.005, &c), "15.00 ms");
        // ticks stay in one unit even when their absolute value is large
        assert_eq!(format_tick(11.234, 0.0005, &c), "11234.00 ms");
        // sub-nanosecond steps still render in ns — but integer ns only, since
        // a fractional `0.30 ns` would drop below the nanosecond floor.
        assert_eq!(format_tick(3.4e-9, 3e-10, &c), "3 ns");
        assert_eq!(format_tick(3e-10, 7e-11, &c), "0 ns");
    }

    #[test]
    fn decimals_are_configurable() {
        let c = TimeFormatConfig {
            decimals: 0,
            ..Default::default()
        };
        assert_eq!(format_duration(90.0, &c), "2 min");
        assert_eq!(format_duration(2.0, &c), "2 s");
        let c = TimeFormatConfig {
            decimals: 3,
            ..Default::default()
        };
        assert_eq!(format_duration(0.0002, &c), "0.200 ms");
    }

    #[test]
    fn minutes_require_a_whole_minute_of_scale() {
        let c = TimeFormatConfig::default();
        assert_eq!(format_duration(1.5, &c), "1.50 s");
        assert_eq!(format_duration(58.0, &c), "58.00 s");
        assert_eq!(format_duration(90.0, &c), "1.50 min");
    }
}
