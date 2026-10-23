//! The way to post work from other threads to the UI thread.
//!
//! `MainContext::invoke` is not used. As glib 0.20's docs say, when the caller owns
//! the main context, **or when nobody owns it**, it runs synchronously on the spot.
//! Called from a worker thread where the main loop is not running (in tests, for
//! example), it would run on that worker thread and never reach the UI thread's
//! registry.
//!
//! `idle_add_once` is always deferred as an idle source of the default main
//! context, which satisfies [`MainThread`]'s promise of "always deferred".

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;

use gtk::glib;
use naui_core::{MainThread, Work};

/// The default main context = the main thread's main loop. There is no handle to
/// carry around, so it has no fields.
pub(crate) struct Idle;

impl MainThread for Idle {
    fn post(&self, work: Work) -> bool {
        glib::idle_add_once(move || {
            // GLib のメインループは C なので、ここから巻き戻すと未定義動作になる。
            // 内容は既定の panic hook が stderr へ出す。
            let _ = catch_unwind(AssertUnwindSafe(work));
        });
        true
    }

    fn post_after(&self, delay: Duration, work: Work) -> bool {
        // `timeout_add_once` も既定のメインコンテキストへ積む。
        glib::timeout_add_once(delay, move || {
            let _ = catch_unwind(AssertUnwindSafe(work));
        });
        true
    }
}
