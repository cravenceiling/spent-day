mod app;
mod collector;
mod colors;
mod day;
mod dbus;
mod demo;
mod desktop;
mod format;
mod idle;
mod layout;
mod media;
mod paths;
mod session;
mod store;
mod tracker;
mod ui;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
