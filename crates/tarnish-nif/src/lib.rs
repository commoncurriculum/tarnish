//! The base of a Rustler NIF on tarnish, shared by tarnish's own (`tarnish_elixir`) and an
//! application's:
//!
//! - [`term`]: terms read as the JSON values Jason encodes them as, and values and nodes made into
//!   the terms Jason decodes from their JSON, with [`etf`] for the terms the VM reads and makes
//!   whole;
//! - [`view`]: terms read in place as that JSON, for a node to be read straight from them into
//!   its chunk;
//! - [`convert`]: `Tarnish.Bridge`'s `convert/1` and `convert_light/1`, which answer its requests
//!   with the conversions an application's NIF serves;
//! - [`light`] and [`Budget`]: calls on the caller's own scheduler, which answer `:dirty` when
//!   they have more work than that takes on;
//! - tarnish's ProseMirror functions, which `Tarnish` calls;
//! - mimalloc as the NIF's allocator, unless the `mimalloc` feature is off;
//! - [`parity`], with the `parity` feature: the tools that hold an application's NIF to its Node
//!   worker, for its examples.
//!
//! A crate that depends on this one has its `rustler::init!` register those functions with its
//! own, so the module that loads its NIF declares them (`use Tarnish.NIF`), and its load hook
//! calls [`load`].

#![forbid(unsafe_code)]

pub mod convert;
mod doc;
pub mod etf;
#[cfg(feature = "parity")]
pub mod parity;
mod prosemirror;
mod share;
pub mod term;
pub mod view;

use std::time::{Duration, Instant};

use rustler::{Encoder, Env, Term};

use convert::Conversions;

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

/// A NIF's `load`: `use Tarnish.NIF` gives the count of threads a batch of conversions runs on as
/// the load info, and an application's NIF serves its conversions, which `convert/1` and
/// `convert_light/1` answer with.
pub fn load(_env: Env, threads: Term, conversions: Option<&'static dyn Conversions>) -> bool {
    let Ok(threads) = threads.decode::<usize>() else {
        return false;
    };
    if let Some(conversions) = conversions {
        convert::serve(conversions, threads);
    }
    true
}

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
