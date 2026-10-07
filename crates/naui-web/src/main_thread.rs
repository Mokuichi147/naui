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

    fn post_after(&self, delay: std::time::Duration, work: Work) -> bool {
        use wasm_bindgen::JsCast;

        let Some(window) = web_sys::window() else {
            return false;
        };
        // `setTimeout` の待ち時間は i32 のミリ秒。それより長いものは丸める
        // (ブラウザも 2^31-1 を超えると即座に呼んでしまうため)。
        let millis = delay.as_millis().min(i32::MAX as u128) as i32;
        let callback = wasm_bindgen::closure::Closure::once_into_js(work);
        window
            .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), millis)
            .is_ok()
    }
}
