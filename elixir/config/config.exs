import Config

# The tests are an application with a NIF of its own, whose conversions test/support/conversions.mjs
# makes in JavaScript.
if config_env() == :test do
  config :tarnish, native: Tarnish.TestNative

  config :tarnish, Tarnish.Bridge,
    conversions: Path.expand("../test/support/conversions.mjs", __DIR__)
end
