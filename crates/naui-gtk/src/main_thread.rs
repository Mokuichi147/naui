//! 別スレッドから UI スレッドへ仕事を投げる口。
//!
//! `MainContext::invoke` は使わない。glib 0.20 の doc にあるとおり、
//! 呼び出し元がメインコンテキストの所有者のとき、**または所有者が誰もいないとき**、
//! その場で同期実行してしまう。メインループを回していない場面 (テストなど) で
//! ワーカースレッドから呼ぶと、そのワーカースレッド上で走ってしまい、
//! UI スレッドの登録簿へ届かない。
//!
//! `idle_add_once` は必ず既定のメインコンテキストの idle source として
//! 遅延されるので、「必ず後回し」という [`MainThread`] の約束を満たす。

use std::panic::{catch_unwind, AssertUnwindSafe};

use gtk::glib;
use naui_core::{MainThread, Work};

/// 既定のメインコンテキスト = メインスレッドのメインループ。
/// ハンドルを持ち歩く必要がないのでフィールドは無い。
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
}
