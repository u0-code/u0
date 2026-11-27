//! u0 with no window, for a machine where nothing is drawn (a Linux
//! box): it runs the agents, keeps the sessions, and serves them to a desk
//! elsewhere over a link (`u0_core::link`).

fn main() {
    u0_core::cli::when_asked();
    u0_core::path::widen();
    let kept = u0_core::settings::Settings::load(&u0_core::settings::home()).0;
    u0_core::logs::start(kept.log_days);
    if let Err(why) = u0_core::link::serve_here() {
        eprintln!("{why}");
        std::process::exit(1);
    }
}
