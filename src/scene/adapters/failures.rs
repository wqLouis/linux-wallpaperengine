//! The frames that could not be drawn.
//!
//! An adapter counts the frames it could not draw and tells the host about
//! them: a frame that cannot be rendered is a failure the user must know
//! about, not something that is counted in silence.
//!
//! The failure is reported once, not once per frame: a wallpaper that
//! cannot be rendered at all fails thousands of times a second and the
//! channel must not be flooded with the same failure.  The wallpaper keeps
//! running: a frame that fails is a problem, not a fatal error.
//!
//! The reporter is the part of the adapters that can be tested without a
//! display: it holds the counter and the channel, nothing else.

use std::sync::mpsc::Sender;

use crate::engine::runtime::EngineStatus;
use crate::scene::error::{EngineProblem, RenderError};

/// Counts the frames that could not be drawn and reports the failure.
pub struct RenderFailures {
    /// Where the failure is reported.
    ///
    /// Without a channel the failures are only counted and written to the
    /// log.
    status: Option<Sender<EngineStatus>>,
    /// The frames that could not be drawn.
    failures: u64,
    /// The failure that was reported already.
    reported: Option<RenderError>,
}

impl RenderFailures {
    /// The reporter of the failures of an adapter.
    pub fn new(status: Option<Sender<EngineStatus>>) -> Self {
        Self {
            status,
            failures: 0,
            reported: None,
        }
    }

    /// The frames that could not be drawn so far.
    pub fn count(&self) -> u64 {
        self.failures
    }

    /// Records a frame that could not be drawn and reports the failure.
    ///
    /// The failure is reported once: the user does not have to be told
    /// about every single frame that was skipped.  Returns `true` when
    /// the failure was reported, that is the first time it happened.
    pub fn record(&mut self, error: RenderError, title: Option<&str>) -> bool {
        self.failures = self.failures.wrapping_add(1);

        if self.reported == Some(error) {
            // The failure is reported already: it is only counted, and
            // the log stays quiet so a broken wallpaper does not fill it.
            log::debug!(
                "the frame could not be drawn: {error} ({} frames failed so far)",
                self.failures
            );
            return false;
        }
        self.reported = Some(error);

        log::warn!(
            "the frame could not be drawn: {error} ({} frames failed so far)",
            self.failures
        );

        let problem = EngineProblem::Render(error);
        if let Some(status) = &self.status {
            // The host shows the problem to the user: the wallpaper keeps
            // running, the problem is only a warning.
            let _ = status.send(EngineStatus::Running {
                title: title.map(str::to_string),
                problems: vec![problem],
            });
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, channel};

    /// The problems of the statuses that arrived.
    fn problems(statuses: &Receiver<EngineStatus>) -> Vec<EngineProblem> {
        statuses
            .try_iter()
            .filter_map(|status| match status {
                EngineStatus::Running { problems, .. } => Some(problems),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// The frame that could not be drawn is reported to the host: the
    /// count alone would tell nobody.
    #[test]
    fn a_frame_that_could_not_be_drawn_is_reported() {
        let (sender, statuses) = channel();
        let mut failures = RenderFailures::new(Some(sender));

        assert!(failures.record(RenderError::Surface, Some("A wallpaper")));

        let reported = problems(&statuses);
        assert_eq!(reported.len(), 1);
        assert!(
            matches!(reported[0], EngineProblem::Render(RenderError::Surface)),
            "unexpected problem: {:?}",
            reported[0]
        );
    }

    /// The failure is reported once, not once per frame: a broken
    /// wallpaper fails thousands of times a second.
    #[test]
    fn the_failure_is_reported_once() {
        let (sender, statuses) = channel();
        let mut failures = RenderFailures::new(Some(sender));

        for _ in 0..1000 {
            failures.record(RenderError::Surface, Some("A wallpaper"));
        }

        // The frames are counted…
        assert_eq!(failures.count(), 1000);
        // …and the failure is reported once.
        assert_eq!(problems(&statuses).len(), 1);
    }

    /// Another failure is reported: the user has to know about it too.
    #[test]
    fn another_failure_is_reported_as_well() {
        let (sender, statuses) = channel();
        let mut failures = RenderFailures::new(Some(sender));

        assert!(failures.record(RenderError::Surface, None));
        assert!(failures.record(RenderError::NotReady, None));
        assert!(!failures.record(RenderError::NotReady, None));

        assert_eq!(problems(&statuses).len(), 2);
    }

    /// Without a channel the failures are only counted.
    #[test]
    fn the_failures_are_counted_without_a_channel() {
        let mut failures = RenderFailures::new(None);

        assert!(failures.record(RenderError::Surface, None));
        assert_eq!(failures.count(), 1);
    }
}
