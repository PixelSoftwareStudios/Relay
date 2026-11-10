//! Translations for the desktop UI, on Project Fluent.
//!
//! Every user-visible string is a stable semantic message id (for example
//! `settings-hotkey-title`) looked up with [`tr!`](crate::tr). The English
//! catalog is the reference and the fallback for any message another language
//! lacks. See `docs/i18n.md`.

mod catalog;
pub mod check;
mod embed;
mod lang;
pub mod locale;
pub mod picker;

use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

pub use catalog::{Catalogs, Value};
pub use embed::{sections as embedded_sections, source as embedded_source};
pub use lang::Language;

static ACTIVE: AtomicUsize = AtomicUsize::new(0);

thread_local! {
	/// A thread's own language (tests only; see [`isolate_thread`]).
	static OWN: Cell<Option<usize>> = const { Cell::new(None) };
}

fn slot() -> &'static RwLock<Arc<Catalogs>> {
	static SLOT: OnceLock<RwLock<Arc<Catalogs>>> = OnceLock::new();
	SLOT.get_or_init(|| RwLock::new(Arc::new(Catalogs::embedded())))
}

fn catalogs() -> Arc<Catalogs> {
	slot()
		.read()
		.map_or_else(|e| e.into_inner().clone(), |g| g.clone())
}

/// Replace the catalogs (a test installs a fake translation; the app never
/// does).
pub fn install(catalogs: Catalogs) {
	*slot().write().unwrap_or_else(|e| e.into_inner()) = Arc::new(catalogs);
}

/// The language the UI is drawn in. English until the shell says otherwise.
pub fn active() -> Language {
	let index = OWN
		.with(Cell::get)
		.unwrap_or_else(|| ACTIVE.load(Ordering::Relaxed));
	Language::ALL[index.min(Language::ALL.len() - 1)]
}

/// Switch the language; the next frame draws in it.
pub fn set_active(language: Language) {
	if OWN.with(|own| own.get().is_some()) {
		OWN.with(|own| own.set(Some(language.index())));
	} else {
		ACTIVE.store(language.index(), Ordering::Relaxed);
	}
}

/// Give the calling thread its own language, so tests that run in one process
/// on parallel threads cannot switch each other's. Afterwards `set_active` and
/// `active` on this thread touch only this thread. Test harnesses only; the app
/// never calls it.
#[doc(hidden)]
pub fn isolate_thread() {
	OWN.with(|own| own.set(Some(ACTIVE.load(Ordering::Relaxed))));
}

/// An argument-free message in the active language. Cheap (a cache hit) and
/// allocation-free after the first call, so it is fine every frame.
pub fn t(id: &'static str) -> &'static str {
	catalogs().text(active(), id)
}

/// A message with arguments, formatted per call.
pub fn t_with(id: &str, args: &[(&str, Value<'_>)]) -> String {
	catalogs().text_with(active(), id, args)
}

/// Look a message up: `tr!("id")` gives a `&'static str`, `tr!("id", n = 3,
/// name = who)` gives a `String`. The id must be a string literal so the
/// catalog tests can find it.
#[macro_export]
macro_rules! tr {
	($id:literal) => {
		$crate::i18n::t($id)
	};
	($id:literal, $($name:ident = $value:expr),+ $(,)?) => {
		$crate::i18n::t_with(
			$id,
			&[$((stringify!($name), $crate::i18n::Value::from($value))),+],
		)
	};
}
