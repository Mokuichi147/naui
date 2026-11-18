# signup

A sign-up form whose UI is tested on all four platforms with `naui::testing`.

```sh
cargo run -p signup
cargo test -p signup
cargo test -p signup --target wasm32-unknown-unknown   # in a browser
```

The UI is built by a library function (`signup::build`) that returns the widgets,
so the tests in `tests/ui.rs` can operate them. The tests click the buttons and the
check box the way a user does, and resize the window to check that the form lines
up its parts when there is room and that the sidebar collapses in a narrow window.

The test target sets `harness = false` and lists its cases with
`naui::test_cases!`. See *Testing apps* in naui's guide.
