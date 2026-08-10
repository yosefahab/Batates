//! Deciding whether this session can host a desktop pet at all.
//!
//! An overlay needs to sit above other windows, cover the output, and pass
//! clicks through except over the pet. Core Wayland (xdg-shell) permits none of
//! that: a client cannot raise itself, cannot place itself, and cannot see the
//! pointer outside its own surface. The only protocol that allows it is
//! `zwlr_layer_shell_v1`.
//!
//! The check is for the protocol itself, not the desktop's name. Names lie:
//! `XDG_CURRENT_DESKTOP` is user-overridable and reads `GNOME:ubuntu` on some
//! systems, and a blocklist would need updating for every new compositor.
//! Asking the registry what it advertises is the truth, and it costs one
//! roundtrip we need anyway.

use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};

/// The protocol that makes an overlay possible.
const LAYER_SHELL: &str = "zwlr_layer_shell_v1";

/// What the session can support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCheck {
    /// Layer shell is available; the overlay can be created.
    Ok,
    /// A Wayland session whose compositor lacks layer shell.
    NoLayerShell { compositor: String },
    /// Not a Wayland session at all.
    NotWayland { display: Option<String> },
    /// No display server reachable.
    NoDisplay { reason: String },
}

/// Collects the interfaces the compositor advertises.
#[derive(Default)]
struct Globals {
    interfaces: Vec<String>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
    fn event(
        state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { interface, .. } = event {
            state.interfaces.push(interface);
        }
    }
}

/// Asks the compositor what it supports.
pub fn check_session() -> SessionCheck {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        let display = std::env::var("DISPLAY").ok();
        return SessionCheck::NotWayland { display };
    }

    let connection = match Connection::connect_to_env() {
        Ok(connection) => connection,
        Err(error) => {
            return SessionCheck::NoDisplay {
                reason: error.to_string(),
            };
        }
    };

    let mut queue = connection.new_event_queue();
    let handle = queue.handle();
    connection.display().get_registry(&handle, ());

    let mut globals = Globals::default();
    if let Err(error) = queue.roundtrip(&mut globals) {
        return SessionCheck::NoDisplay {
            reason: error.to_string(),
        };
    }

    if globals.interfaces.iter().any(|name| name == LAYER_SHELL) {
        SessionCheck::Ok
    } else {
        SessionCheck::NoLayerShell {
            compositor: compositor_name(),
        }
    }
}

/// A human-readable name for the desktop, used only in error messages.
///
/// Never used for the decision itself, because it can be set to anything.
fn compositor_name() -> String {
    for key in [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ] {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            return value;
        }
    }
    "unknown".to_string()
}

/// The message shown when the session cannot host the overlay.
///
/// Kept a pure function of the check so its wording is testable and so the
/// caller decides where it goes.
pub fn explain(check: &SessionCheck) -> String {
    match check {
        SessionCheck::Ok => String::new(),
        SessionCheck::NoLayerShell { compositor } => format!(
            "batates: your compositor does not support wlr-layer-shell.\n\
             \n\
             \x20 Detected desktop: {compositor}\n\
             \x20 Required protocol: {LAYER_SHELL} (not advertised)\n\
             \n\
             batates draws itself as an always-on-top, click-through overlay. On\n\
             Wayland the only protocol that permits this is {LAYER_SHELL}.\n\
             GNOME/Mutter has declined to implement it, so batates cannot run\n\
             there, and there is no workaround short of GNOME adopting it.\n\
             \n\
             Known-working compositors: Sway, Hyprland, river, niri,\n\
             KDE Plasma 6 (KWin), COSMIC."
        ),
        SessionCheck::NotWayland { display } => {
            let session = match display {
                Some(display) => format!("an X11 session (DISPLAY={display})"),
                None => "no Wayland session".to_string(),
            };
            format!(
                "batates: this is {session}, and WAYLAND_DISPLAY is unset.\n\
                 \n\
                 batates supports Wayland only on Linux. Log in to a Wayland\n\
                 session on a supported compositor and try again."
            )
        }
        SessionCheck::NoDisplay { reason } => format!(
            "batates: could not reach the Wayland compositor: {reason}\n\
             \n\
             WAYLAND_DISPLAY is set but the socket could not be opened."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_needs_no_explanation() {
        assert!(explain(&SessionCheck::Ok).is_empty());
    }

    /// The message must name the missing protocol and point at compositors that
    /// work; a bare "unsupported" leaves the user with nowhere to go.
    #[test]
    fn missing_layer_shell_names_the_protocol_and_alternatives() {
        let message = explain(&SessionCheck::NoLayerShell {
            compositor: "GNOME".to_string(),
        });
        assert!(message.contains(LAYER_SHELL));
        assert!(message.contains("GNOME"));
        assert!(message.contains("Sway"));
        assert!(message.contains("Hyprland"));
    }

    #[test]
    fn an_x11_session_is_reported_distinctly() {
        let message = explain(&SessionCheck::NotWayland {
            display: Some(":0".to_string()),
        });
        assert!(message.contains("DISPLAY=:0"));
        assert!(
            !message.contains(LAYER_SHELL),
            "an X11 session is not a layer-shell problem"
        );
    }

    #[test]
    fn an_unreachable_compositor_reports_the_cause() {
        let message = explain(&SessionCheck::NoDisplay {
            reason: "No such file or directory".to_string(),
        });
        assert!(message.contains("No such file or directory"));
    }
}
