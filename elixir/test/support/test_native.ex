defmodule Tarnish.TestNative do
  @moduledoc false

  # The NIF of the application tarnish's tests make, on tarnish-nif: tarnish's functions, and
  # conversions.mjs's conversions in Rust.
  use Tarnish.NIF,
    otp_app: :tarnish,
    crate: :tarnish_test_native,
    path: "test/support/test_native"
end
