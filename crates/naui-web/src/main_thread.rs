//! The way to post work from other threads to the UI thread.
//!
//! wasm has no threads, so "another thread" does not apply, but the promise of
//! "always deferring" is shared by all four backends, so in the browser too one
//! microtask queue hop is inserted.
//!
//! `js_sys::futures::spawn_local` guarantees that "even a future that returns
//! `Ready` immediately always runs on the next microtask", internally using
//! `queueMicrotask` (or Promise resolution when unavailable). It can be trusted
//! with the availability check too, which is simpler than handling `setTimeout`
//! ourselves.

use naui_core::{MainThread, Work};

/// The microtask queue. Browsers have no app shutdown, so posting always succeeds.
///
/// wasm uses `panic = "abort"` by default, so panics cannot be caught here.
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
