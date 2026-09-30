//! The NIF `Tarnish.Native` loads: tarnish-nif's ProseMirror functions, which an application's
//! own NIF holds instead when it builds on tarnish-nif (`Tarnish.NIF`).

#![forbid(unsafe_code)]

use tarnish_nif as _;

rustler::init!("Elixir.Tarnish.Native", load = tarnish_nif::load);
