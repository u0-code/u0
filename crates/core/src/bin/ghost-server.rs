//! Ghost with no window, for a machine where nothing is drawn (a Linux
//! box): it runs the agents, keeps the sessions, and serves them to a desk
//! elsewhere over a link (`ghost_core::link`).

fn main() {
    ghost_core::cli::when_asked();
    ghost_core::path::widen();
    let kept = ghost_core::settings::Settings::load(&ghost_core::settings::home()).0;
    ghost_core::logs::start(kept.log_days);
    if let Err(why) = ghost_core::link::serve_here() {
        eprintln!("{why}");
        std::process::exit(1);
    }
}
