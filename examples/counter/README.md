# counter

The minimal naui example. It shows a counter built from nothing but a native
label and buttons.

```sh
cargo run -p counter
```

It covers the basic shape of a naui app: build the UI inside the callback passed
to `run`, and update the label from `on_click`.
