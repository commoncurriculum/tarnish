//! The base of a Rustler NIF on tarnish, shared by tarnish's own (`tarnish_elixir`) and an
//! application's:
//!
//! - [`term`]: terms read as the JSON values Jason encodes them as, and values and nodes made into
//!   the terms Jason decodes from their JSON, with [`etf`] for the terms the VM reads and makes
//!   whole;
//! - [`light`] and [`Budget`]: calls on the caller's own scheduler, which answer `:dirty` when
//!   they have more work than that takes on;
//! - [`pool`]: threads for a batch's work, one per dirty CPU scheduler, which [`load`] starts;
//! - tarnish's ProseMirror functions, which `Tarnish` calls.
//!
//! A crate that depends on this one has its `rustler::init!` register those functions with its
//! own, so the module that loads its NIF declares them (`use Tarnish.NIF`).

#![forbid(unsafe_code)]

mod doc;
pub mod etf;
mod prosemirror;
mod share;
pub mod term;
mod view;

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use rustler::{Encoder, Env, Term};

mod atoms {
    rustler::atoms! {
        dirty,
    }
}

/// What a call may take on: its terms' weight, as [`term::Reader`] weighs them, and its time,
/// for work that checks the time as it goes.
#[derive(Clone, Copy)]
pub struct Budget {
    pub weight: usize,
    pub time: Option<Duration>,
}

/// A light call's: about half a millisecond, well within the millisecond a process's timeslice
/// is.
pub const LIGHT: Budget = Budget {
    weight: 8_192,
    time: Some(Duration::from_micros(500)),
};

pub const UNLIMITED: Budget = Budget {
    weight: usize::MAX,
    time: None,
};

/// Runs a call on the caller's own scheduler. Handing the process to a dirty scheduler and back
/// takes about as long as a light call. The time the call takes counts against the process's
/// timeslice, which the VM balances its schedulers' load by.
pub fn light<'a, T>(env: Env<'a>, call: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let answer = call();
    // A timeslice is about a millisecond.
    let used = started.elapsed().as_micros() / 10;
    rustler::schedule::consume_timeslice(env, used.clamp(1, 100) as i32);
    answer
}

/// A light call's answer when it has more work than it takes on, for the caller to make the call
/// again on a dirty scheduler.
pub fn dirty(env: Env) -> Term {
    atoms::dirty().encode(env)
}

/// The threads a batch's work runs on.
static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();

pub fn pool() -> &'static rayon::ThreadPool {
    POOL.get().expect("the pool starts when the NIF loads")
}

/// A NIF's `load`: starts the pool with as many threads as the load info says, which is the
/// VM's count of dirty CPU schedulers, or with one a core for `0`.
pub fn load(_env: Env, threads: Term) -> bool {
    let Ok(threads) = threads.decode::<usize>() else {
        return false;
    };
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|index| format!("tarnish-{index}"))
            .build()
            .expect("the pool's threads start")
    });
    true
}
