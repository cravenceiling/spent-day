use super::dbus;
use crate::platform::{Instance, OnActivate};
use zbus::blocking::{Connection, connection::Builder};
use zbus::fdo::{RequestNameFlags, RequestNameReply};

const PATH: &str = "/day/spent/SpentDay";
const INTERFACE: &str = "day.spent.SpentDay";

pub struct InstanceGuard {
    _connection: Connection,
}

struct Activation(OnActivate);

#[zbus::interface(name = "day.spent.SpentDay")]
impl Activation {
    fn activate(&self, show: bool, token: String) {
        (self.0)(show, token);
    }
}

pub fn claim_instance(demo: bool, show: bool, on_activate: OnActivate) -> Instance {
    let token = std::env::var("XDG_ACTIVATION_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())
        .or_else(|| std::env::var("DESKTOP_STARTUP_ID").ok())
        .unwrap_or_default();
    claim(demo, show, token, on_activate).unwrap_or_else(|error| {
        // Continuing without the name could start a second tracker against the same database.
        eprintln!("spentday: could not register or activate the running app: {error}");
        std::process::exit(1)
    })
}

fn claim(demo: bool, show: bool, token: String, on_activate: OnActivate) -> zbus::Result<Instance> {
    let name = if demo {
        "day.spent.SpentDay.Demo"
    } else {
        INTERFACE
    };
    // Register the method before claiming the name so a concurrent launch can call it at once.
    let connection = Builder::session()?
        .method_timeout(dbus::TIMEOUT)
        .serve_at(PATH, Activation(on_activate))?
        .build()?;
    match connection.request_name_with_flags(name, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
            Ok(Instance::Primary(InstanceGuard {
                _connection: connection,
            }))
        }
        Ok(_) | Err(zbus::Error::NameTaken) => {
            dbus::proxy(&connection, name, PATH, INTERFACE)?
                .call::<_, _, ()>("Activate", &(show, token))?;
            Ok(Instance::Forwarded)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    #[ignore = "needs a D-Bus session bus; verify.sh runs it"]
    fn second_claim_forwards_show_and_token() {
        let (activated, activations) = mpsc::channel();
        let first = claim(
            false,
            false,
            String::new(),
            Box::new(move |show, token| {
                activated.send((show, token)).unwrap();
            }),
        )
        .unwrap();
        let Instance::Primary(guard) = first else {
            panic!("first claim was forwarded")
        };
        let second = claim(
            false,
            true,
            "tok".into(),
            Box::new(|_, _| panic!("second became primary")),
        )
        .unwrap();
        assert!(matches!(second, Instance::Forwarded));
        assert_eq!(
            activations.recv_timeout(dbus::TIMEOUT).unwrap(),
            (true, "tok".into())
        );
        let background = claim(
            false,
            false,
            "background".into(),
            Box::new(|_, _| panic!("background became primary")),
        )
        .unwrap();
        assert!(matches!(background, Instance::Forwarded));
        assert_eq!(
            activations.recv_timeout(dbus::TIMEOUT).unwrap(),
            (false, "background".into())
        );
        let demo = claim(true, true, String::new(), Box::new(|_, _| {})).unwrap();
        assert!(matches!(demo, Instance::Primary(_)));
        let other = dbus::session().unwrap();
        let replaced = other.request_name_with_flags(
            INTERFACE,
            RequestNameFlags::ReplaceExisting | RequestNameFlags::DoNotQueue,
        );
        assert!(matches!(
            replaced,
            Ok(RequestNameReply::Exists) | Err(zbus::Error::NameTaken)
        ));
        drop(guard);
        assert!(matches!(
            claim(false, false, String::new(), Box::new(|_, _| {})).unwrap(),
            Instance::Primary(_)
        ));
    }
}
