//! The way to post work from other threads to the UI thread.
//!
//! AppKit has no API for posting a closure to the main thread, so libdispatch's
//! main queue is used. `dispatch_async` items are taken out in order while the main
//! loop runs, which satisfies [`MainThread`]'s promise of "always deferred" as is.

use std::panic::{catch_unwind, AssertUnwindSafe};

use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use naui_core::{MainThread, Work};

/// The main queue. `DispatchQueue::main()` returns a `&'static`, so there is no
/// handle to carry around.
pub(crate) struct MainQueue;

impl MainThread for MainQueue {
    fn post(&self, work: Work) -> bool {
        DispatchQueue::main().exec_async(move || {
            // libdispatch は C なので、ここから巻き戻すと未定義動作になる。
            // 内容は既定の panic hook が stderr へ出す。
            let _ = catch_unwind(AssertUnwindSafe(work));
        });
        true
    }

    fn post_after(&self, delay: Duration, work: Work) -> bool {
        // 遠すぎて表せない時間は、事実上来ないのと同じなので積まない。
        let Ok(when) = DispatchTime::try_from(delay) else {
            return false;
        };
        DispatchQueue::main()
            .after(when, move || {
                let _ = catch_unwind(AssertUnwindSafe(work));
            })
            .is_ok()
    }
}
