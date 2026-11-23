# charts

Line, bar and scatter charts drawn with [naui-charts](../../crates/naui-charts)
on naui's `Canvas`.

```sh
cargo run -p charts
```

The window shows bars and a line in one chart over named months, a scatter chart
of two groups, and a line of 100,000 points, which naui-charts thins to about four
points per pixel column. "Dark appearance" switches the theme; the charts draw
themselves again in the colors for it. "Add a month" changes a series through
`ChartView::update`.

To run it on the Web, build it with `web/build.sh` (it needs the same
`wasm-bindgen-cli` as the Gallery) and serve `web/` over HTTP.

```sh
cd examples/charts/web
./build.sh
python3 -m http.server 8080
```
