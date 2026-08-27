//! 別スレッドから UI スレッドへ仕事を投げる口。
//!
//! wasm にはスレッドが無いので「別スレッド」は成立しないが、
//! 「必ず後回しにする」という約束は 4 バックエンドで共通なので、
//! ブラウザでも microtask キューを 1 つ挟む。
//!
//! `js_sys::futures::spawn_local` は「即座に `Ready` を返す future でも
//! 必ず次の microtask で走る」と保証されており、内部は `queueMicrotask`
//! (無ければ Promise の解決) を使う。可用性の判定ごと任せられるので、
//! `setTimeout` を自前で扱うより素直。

use naui_core::{MainThread, Work};

/// microtask キュー。ブラウザにはアプリの終了が無いので、投函は必ず成功する。
///
/// wasm は既定で `panic = "abort"` なので、ここで panic を捕まえることはできない。
pub(crate) struct Microtask;

impl MainThread for Microtask {
    fn post(&self, work: Work) -> bool {
        js_sys::futures::spawn_local(async move { work() });
        true
    }
}
