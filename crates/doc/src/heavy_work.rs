//! Work too slow for a build that computes on the page's own thread (the browser): the app turns
//! it off there, and the model shows a lighter stand-in with a note instead of freezing the page.
//! A setting of the running app, not design data: the design keeps what was asked for, and the
//! desktop app builds it in full. Per thread: the browser computes on its one thread, and a test
//! that turns it off leaves the others alone.

use std::cell::Cell;

thread_local! {
    static MODELLED_THREADS: Cell<bool> = const { Cell::new(true) };
}

/// Are modelled threads cut into the body? When off, a modelled thread is shown as a cosmetic one.
pub fn modelled_threads() -> bool {
    MODELLED_THREADS.with(Cell::get)
}

/// Turn cutting modelled threads on or off (the browser build turns it off at start).
pub fn set_modelled_threads(on: bool) {
    MODELLED_THREADS.with(|c| c.set(on));
}
