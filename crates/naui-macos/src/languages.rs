//! Following changes of the OS's preferred languages.
//!
//! Changing the region, or the per-app language in System Settings, posts
//! `NSCurrentLocaleDidChangeNotification`, which is when `NSLocale.preferredLanguages`
//! is read again. The notification can arrive on any thread, so the reading is moved
//! to the main queue, where every live `Ui` takes the new list.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, sel, AnyThread};
use objc2_foundation::{
    NSCurrentLocaleDidChangeNotification, NSLocale, NSNotification, NSNotificationCenter,
};

use crate::UiInner;

thread_local! {
    /// The `Ui`s waiting for changes. Tests make one per case, so dead ones are
    /// dropped when the next one registers.
    static WATCHERS: RefCell<Vec<Weak<UiInner>>> = const { RefCell::new(Vec::new()) };
    /// Whether the observer is registered with the notification center. One for the
    /// whole process, never removed.
    static OBSERVING: Cell<bool> = const { Cell::new(false) };
    /// What [`preferred_languages`] returns instead of the OS's, for tests.
    static OVERRIDE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

define_class!(
    /// Receives `NSCurrentLocaleDidChangeNotification`. It holds nothing, so it can
    /// be called on any thread.
    #[unsafe(super(NSObject))]
    #[name = "NauiLocaleObserver"]
    struct LocaleObserver;

    unsafe impl NSObjectProtocol for LocaleObserver {}

    impl LocaleObserver {
        #[unsafe(method(currentLocaleDidChange:))]
        fn current_locale_did_change(&self, _notification: &NSNotification) {
            DispatchQueue::main().exec_async(refresh);
        }
    }
);

impl LocaleObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Starts telling `ui` about changes. Called on the main thread.
pub(crate) fn watch(ui: &Rc<UiInner>) {
    WATCHERS.with(|watchers| {
        let mut watchers = watchers.borrow_mut();
        watchers.retain(|watcher| watcher.strong_count() != 0);
        watchers.push(Rc::downgrade(ui));
    });
    if OBSERVING.replace(true) {
        return;
    }
    let observer = LocaleObserver::new();
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &observer,
            sel!(currentLocaleDidChange:),
            Some(NSCurrentLocaleDidChangeNotification),
            None,
        );
    }
    // 通知センターは観測者を強く持たない。プロセスの終わりまで外さないので手放す。
    std::mem::forget(observer);
}

/// Reads the languages again and hands them to every live `Ui`.
fn refresh() {
    let watchers: Vec<Rc<UiInner>> =
        WATCHERS.with(|watchers| watchers.borrow().iter().filter_map(Weak::upgrade).collect());
    if watchers.is_empty() {
        return;
    }
    let languages = preferred_languages();
    // 借用を持たずに知らせる。知らせの中で Ui を作っても登録できる。
    for ui in watchers {
        ui.languages.update(languages.clone());
    }
}

/// The OS's preferred languages, most preferred first (`"ja-JP"` and so on).
///
/// `NSLocale.preferredLanguages` follows the per-app language in System Settings
/// too. AppKit's own strings follow the app bundle's localizations instead, so an
/// app without a bundle gets them in English.
pub(crate) fn preferred_languages() -> Vec<String> {
    if let Some(languages) = OVERRIDE.with(|o| o.borrow().clone()) {
        return languages;
    }
    NSLocale::preferredLanguages()
        .iter()
        .map(|tag| tag.to_string())
        .collect()
}

/// Makes the languages read from now on `languages` instead of the OS's (`None`
/// goes back to the OS). **For automated tests only.**
///
/// Nothing is notified by this alone. Post `NSCurrentLocaleDidChangeNotification`
/// to have them read again.
#[doc(hidden)]
pub fn override_preferred_languages_for_test(languages: Option<Vec<String>>) {
    OVERRIDE.with(|o| *o.borrow_mut() = languages);
}
