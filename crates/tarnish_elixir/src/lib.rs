//! The NIF `Tarnish.Native` loads: tarnish-nif's ProseMirror functions, which an application's
//! own NIF holds instead when it builds on tarnish-nif (`Tarnish.NIF`).

#![forbid(unsafe_code)]

fn load(env: rustler::Env, info: rustler::Term) -> bool {
    tarnish_nif::load(env, info, None)
}

rustler::init!("Elixir.Tarnish.Native", load = load);
