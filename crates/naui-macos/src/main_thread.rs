//! 別スレッドから UI スレッドへ仕事を投げる口。
//!
//! AppKit にはメインスレッドへクロージャを投函する API が無いので、
//! libdispatch のメインキューを使う。`dispatch_async` はメインループが
//! 回っている間に順番に取り出されるため、「必ず後回し」という
//! [`MainThread`] の約束をそのまま満たす。

use std::panic::{catch_unwind, AssertUnwindSafe};

use dispatch2::DispatchQueue;
use naui_core::{MainThread, Work};

/// メインキュー。`DispatchQueue::main()` は `&'static` を返すので、
/// ハンドルを持ち歩く必要がない。
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
}
