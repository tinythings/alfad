# Alfad

Alfad (pronounced "alpha dee") is a minimalistic yet another init
system, written in Rust.

## What it is for?

The reasons for its existence as follows:

- It is written in Rust. So it can be qualified for mission critical
  use cases. One can use [Ferrocene](https://ferrous-systems.com/ferrocene/) — an open source qualified Rust
  compiler toolchain for this purpose.

- It is designed for non-generic, specific preconfigured use
  cases. Therefore it lacks convenient automatism on purpose, because
  such systems do not utilise these features in the first place.

- Systems, those targeted to be run by `alfad` are not even meant to
  have a minimal shell onboard or even any kind of scripting, but be
  100% script-free systems.

- It meant to be as deterministic as possible, with very little jitter
  allowance.


## What it is not?

Don't be misled in the idea: the `alfad` init system is not a
replacement or alternative to `systemd` and never will be. If you are
after big and complex system, such as multimedia desktops, consumer
level setups where you need fully feature-packed automated solutions,
the `alfad` is not for you: just keep `systemd` as is.
