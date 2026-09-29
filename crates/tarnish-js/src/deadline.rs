//! A time limit on a conversion that must be short, such as one on the scheduler of the process
//! calling the NIF. The loops whose turns can each take time in proportion to what they have left,
//! as marked's lexer's do on some texts, check it as they go, and fail once it has passed, and a
//! string whose length a number in the document sets fails before it is built.

use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::JsError;

/// A check's failure once the deadline has passed.
pub struct Late;

thread_local! {
    static AT: Cell<Option<Instant>> = const { Cell::new(None) };
    static MISSED: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f`, or gives `None` if it runs past `limit`. A check that fails makes this `None`
/// however `f` goes on from its failure, so that nothing `f` made of it is taken for a result.
pub fn within<R>(limit: Duration, f: impl FnOnce() -> R) -> Option<R> {
    struct Before(Option<Instant>);
    impl Drop for Before {
        fn drop(&mut self) {
            AT.set(self.0);
            MISSED.set(false);
        }
    }
    let _before = Before(AT.replace(Some(Instant::now() + limit)));
    let value = f();
    (!MISSED.get()).then_some(value)
}

/// The longest string a conversion within a deadline builds in one go. What a conversion builds
/// grows with what it converts, which a light request's weight bounds, but a heading's marker and
/// a list item's indent repeat as many times as a number in the document says.
const LONGEST: usize = 1 << 20;

/// Fails, before any of it is built, if what runs on this thread has a deadline and a string of
/// `length` bytes is longer than `LONGEST`.
pub fn build(length: usize) -> Result<(), Late> {
    if length > LONGEST && AT.get().is_some() {
        MISSED.set(true);
        return Err(Late);
    }
    Ok(())
}

/// How much work passes between reads of the clock: a few microseconds' worth.
const QUANTUM: usize = 1 << 12;

/// The work a turn of a loop takes besides scanning what it has left.
const TURN: usize = 64;

/// The deadline of what runs on this thread, for a loop to check.
pub struct Deadline {
    at: Option<Instant>,
    spent: usize,
}

impl Deadline {
    pub fn current() -> Deadline {
        Deadline {
            at: AT.get(),
            spent: 0,
        }
    }

    /// Notes a turn of a loop that may scan `left` units, such as the text left to lex or the
    /// marks open, and fails once the deadline has passed, and at every turn after.
    #[inline]
    pub fn turn(&mut self, left: usize) -> Result<(), Late> {
        let Some(at) = self.at else {
            return Ok(());
        };
        self.spent += left + TURN;
        if self.spent >= QUANTUM {
            if Instant::now() > at {
                MISSED.set(true);
                return Err(Late);
            }
            self.spent = 0;
        }
        Ok(())
    }
}

/// What a failed check says, which nothing reads: `within` drops what it fails.
const LATE: &str = "The conversion ran past its deadline";

impl From<Late> for JsError {
    fn from(Late: Late) -> JsError {
        JsError::range_error(LATE)
    }
}

impl From<Late> for String {
    fn from(Late: Late) -> String {
        LATE.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spin(work: usize) -> Result<usize, Late> {
        let mut deadline = Deadline::current();
        for turn in 0..work {
            deadline.turn(QUANTUM)?;
            std::hint::black_box(turn);
        }
        Ok(work)
    }

    #[test]
    fn gives_what_finishes_in_time() {
        assert!(matches!(
            within(Duration::from_secs(60), || spin(1_000)),
            Some(Ok(1_000))
        ));
    }

    #[test]
    fn gives_nothing_for_what_runs_past_it() {
        assert!(within(Duration::ZERO, || spin(usize::MAX)).is_none());
        // Even when what ran made something else of the failure.
        assert!(within(Duration::ZERO, || spin(usize::MAX).unwrap_or(0)).is_none());
        // The deadline goes with the call that set it.
        assert!(matches!(spin(1_000), Ok(1_000)));
    }
}
