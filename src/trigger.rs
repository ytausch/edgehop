//! When a cursor resting at an edge should trigger a switch.

use std::time::{Duration, Instant};

use crate::desktop::{Edge, Point};

/// How far the cursor has to move away from the edge after a switch before
/// the next one can trigger.
pub const REARM_DISTANCE: i32 = 50;

pub struct Trigger {
    dwell: Duration,
    cooldown: Duration,
    state: State,
}

enum State {
    /// Waiting for the cursor to rest at an edge, which it has been doing
    /// since the given instant.
    Armed { resting: Option<(Edge, Instant)> },
    /// Fired at `edge` with the cursor at `origin`. Re-arms once the cursor
    /// has moved inland and the cooldown has run out, so that a cursor left
    /// at the edge does not switch straight back when the devices return.
    Disarmed {
        edge: Edge,
        origin: Point,
        until: Instant,
        moved_inland: bool,
    },
}

impl Trigger {
    pub fn new(dwell: Duration, cooldown: Duration) -> Self {
        Self {
            dwell,
            cooldown,
            state: State::Armed { resting: None },
        }
    }

    /// Feeds in one cursor sample and the edge it is at, if any. Returns the
    /// edge when the trigger fires.
    pub fn update(&mut self, now: Instant, cursor: Point, at: Option<Edge>) -> Option<Edge> {
        match &mut self.state {
            State::Disarmed {
                edge,
                origin,
                until,
                moved_inland,
            } => {
                *moved_inland |= edge.inland_distance(*origin, cursor) >= REARM_DISTANCE;
                if *moved_inland && now >= *until {
                    self.state = State::Armed { resting: None };
                }
                None
            }
            State::Armed { resting } => {
                let Some(edge) = at else {
                    *resting = None;
                    return None;
                };
                let since = match *resting {
                    Some((resting_at, since)) if resting_at == edge => since,
                    _ => now,
                };
                if now - since < self.dwell {
                    *resting = Some((edge, since));
                    return None;
                }
                self.state = State::Disarmed {
                    edge,
                    origin: cursor,
                    until: now + self.cooldown,
                    moved_inland: false,
                };
                Some(edge)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DWELL: Duration = Duration::from_millis(250);
    const COOLDOWN: Duration = Duration::from_secs(2);

    /// Drives a trigger with a timeline in milliseconds from a fixed start.
    struct Timeline {
        trigger: Trigger,
        start: Instant,
    }

    impl Timeline {
        fn new() -> Self {
            Self {
                trigger: Trigger::new(DWELL, COOLDOWN),
                start: Instant::now(),
            }
        }

        fn at(&mut self, ms: u64, x: i32, edge: Option<Edge>) -> Option<Edge> {
            let now = self.start + Duration::from_millis(ms);
            self.trigger.update(now, Point { x, y: 500 }, edge)
        }

        /// Fires at the right edge (x = 1919) at 250 ms.
        fn fire(&mut self) {
            assert_eq!(self.at(0, 1919, Some(Edge::Right)), None);
            assert_eq!(self.at(250, 1919, Some(Edge::Right)), Some(Edge::Right));
        }
    }

    #[test]
    fn fires_once_the_cursor_has_rested_for_the_dwell_time() {
        let mut timeline = Timeline::new();
        assert_eq!(timeline.at(0, 1919, Some(Edge::Right)), None);
        assert_eq!(timeline.at(249, 1919, Some(Edge::Right)), None);
        assert_eq!(timeline.at(250, 1919, Some(Edge::Right)), Some(Edge::Right));
    }

    #[test]
    fn fires_immediately_without_dwell_time() {
        let mut trigger = Trigger::new(Duration::ZERO, COOLDOWN);
        let fired = trigger.update(Instant::now(), Point { x: 0, y: 0 }, Some(Edge::Left));
        assert_eq!(fired, Some(Edge::Left));
    }

    #[test]
    fn restarts_the_dwell_time_when_the_cursor_leaves_the_edge() {
        let mut timeline = Timeline::new();
        timeline.at(0, 1919, Some(Edge::Right));
        timeline.at(200, 1900, None);
        assert_eq!(timeline.at(210, 1919, Some(Edge::Right)), None);
        assert_eq!(timeline.at(300, 1919, Some(Edge::Right)), None);
        assert_eq!(timeline.at(460, 1919, Some(Edge::Right)), Some(Edge::Right));
    }

    #[test]
    fn restarts_the_dwell_time_when_the_cursor_moves_to_another_edge() {
        let mut timeline = Timeline::new();
        timeline.at(0, 1919, Some(Edge::Right));
        assert_eq!(timeline.at(200, 1919, Some(Edge::Top)), None);
        assert_eq!(timeline.at(300, 1919, Some(Edge::Top)), None);
        assert_eq!(timeline.at(450, 1919, Some(Edge::Top)), Some(Edge::Top));
    }

    #[test]
    fn stays_disarmed_at_the_edge_after_firing() {
        let mut timeline = Timeline::new();
        timeline.fire();
        assert_eq!(timeline.at(10_000, 1919, Some(Edge::Right)), None);
        assert_eq!(timeline.at(20_000, 1919, Some(Edge::Right)), None);
    }

    #[test]
    fn rearms_after_moving_inland_and_the_cooldown() {
        let mut timeline = Timeline::new();
        timeline.fire();
        timeline.at(1_000, 1919 - REARM_DISTANCE, None);
        timeline.at(1_500, 1000, None);
        // Back at the edge before the cooldown ends: the trip inland counts,
        // but the cooldown still holds.
        timeline.at(2_000, 1919, Some(Edge::Right));
        timeline.at(2_249, 1919, Some(Edge::Right));
        // Re-armed at 2 250 ms; the dwell time starts with the next sample.
        timeline.at(2_250, 1919, Some(Edge::Right));
        assert_eq!(timeline.at(2_300, 1919, Some(Edge::Right)), None);
        assert_eq!(
            timeline.at(2_550, 1919, Some(Edge::Right)),
            Some(Edge::Right)
        );
    }

    #[test]
    fn does_not_rearm_after_moving_less_than_the_distance() {
        let mut timeline = Timeline::new();
        timeline.fire();
        timeline.at(3_000, 1919 - REARM_DISTANCE + 1, None);
        timeline.at(4_000, 1919, Some(Edge::Right));
        assert_eq!(timeline.at(5_000, 1919, Some(Edge::Right)), None);
    }

    #[test]
    fn does_not_rearm_before_the_cooldown() {
        let mut timeline = Timeline::new();
        timeline.fire();
        timeline.at(300, 1000, None);
        timeline.at(2_249, 1919, Some(Edge::Right));
        assert_eq!(timeline.at(2_600, 1919, Some(Edge::Right)), None);
    }
}
