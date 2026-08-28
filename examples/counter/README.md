# counter

naui の最小サンプルです。ネイティブのラベルとボタンだけを使ったカウンターを
表示します。

```sh
cargo run -p counter
```

`run` に渡すコールバックの中で UI を組み立て、`on_click` でラベルを書き換える
という naui の基本形をひと通り含んでいます。
