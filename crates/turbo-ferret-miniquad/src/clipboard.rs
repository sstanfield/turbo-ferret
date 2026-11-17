//! The system clipboard for a miniquad app.
//!
//! miniquad 0.4.11 has a clipboard on Windows, X11 and the web, but its macOS one is
//! a stub: `clipboard_set` does nothing and `clipboard_get` returns `None`. So on
//! macOS this goes through the system's `pbcopy` and `pbpaste` tools instead, which
//! needs no Objective-C binding (and no `unsafe`). Everywhere, the last text set is
//! also kept in memory, so copy and paste inside the app work even where the system
//! clipboard cannot be reached.

/// The system clipboard. Give it to the `Ui` once miniquad is running:
/// `ui.services_mut().clipboard = Box::new(MiniquadClipboard::new())`.
#[derive(Default)]
pub struct MiniquadClipboard {
    /// The last text set here, for when the system clipboard cannot be reached.
    local: String,
}

impl MiniquadClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl turbo_ferret::Clipboard for MiniquadClipboard {
    fn set_text(&mut self, text: &str) {
        self.local = text.to_string();
        system::set(text);
    }

    fn text(&mut self) -> Option<String> {
        choose(system::get(), &self.local)
    }
}

/// What the system clipboard said.
#[derive(Debug, PartialEq)]
enum SystemText {
    /// This text (not empty).
    Text(String),
    /// It was asked and holds no text. (Only the macOS route can tell.)
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Empty,
    /// It could not be asked, or it does not tell "empty" from "not there".
    Unknown,
}

/// The system's text wins. Our own copy is used only when the system clipboard could
/// not be asked: if it answered "no text", something else (an image, say) was copied
/// since, and pasting our old text would be wrong.
fn choose(system: SystemText, local: &str) -> Option<String> {
    match system {
        SystemText::Text(t) => Some(t),
        SystemText::Empty => None,
        SystemText::Unknown => (!local.is_empty()).then(|| local.to_string()),
    }
}

#[cfg(target_os = "macos")]
mod system {
    use super::SystemText;
    use std::io::Write;
    use std::process::{Command, Stdio};

    /// Both tools convert with the locale's encoding, and an app started from the
    /// Finder has no locale: without this, non-ASCII text turns into Mac Roman and
    /// question marks.
    fn tool(path: &str) -> Command {
        let mut c = Command::new(path);
        // (LC_ALL would override LC_CTYPE.)
        c.env_remove("LC_ALL").env("LC_CTYPE", "UTF-8");
        c
    }

    pub fn set(text: &str) {
        let child = tool("/usr/bin/pbcopy").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        if let Ok(mut child) = child {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
                // Dropping stdin closes it, which is what pbcopy waits for.
            }
            let _ = child.wait();
        }
    }

    pub fn get() -> SystemText {
        match tool("/usr/bin/pbpaste").stdin(Stdio::null()).stderr(Stdio::null()).output() {
            // (The clipboard answered: whatever it holds, it is not our old text.)
            Ok(out) if out.status.success() => match String::from_utf8_lossy(&out.stdout) {
                t if !t.is_empty() => SystemText::Text(t.into_owned()),
                _ => SystemText::Empty,
            },
            _ => SystemText::Unknown,
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod system {
    use super::SystemText;

    pub fn set(text: &str) {
        miniquad::window::clipboard_set(text);
    }

    /// miniquad answers `None` both for an empty clipboard and for a platform
    /// without one, so `None` is "unknown" here.
    pub fn get() -> SystemText {
        match miniquad::window::clipboard_get() {
            Some(t) if !t.is_empty() => SystemText::Text(t),
            _ => SystemText::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_text_wins_and_local_is_the_fallback() {
        assert_eq!(choose(SystemText::Text("sys".into()), "local").as_deref(), Some("sys"));
        assert_eq!(choose(SystemText::Unknown, "local").as_deref(), Some("local"));
        assert_eq!(choose(SystemText::Unknown, ""), None);
        // The system clipboard holds something that is not text: our copy is stale.
        assert_eq!(choose(SystemText::Empty, "local"), None);
    }

    /// Cut and paste in an edit field through the real macOS clipboard. Ignored by
    /// default because it writes the system clipboard (it puts the old text back).
    /// Run it with `cargo test -p turbo-ferret-miniquad -- --ignored`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "writes the system clipboard"]
    fn macos_cut_and_paste_round_trip() {
        use turbo_ferret::widgets::EditField;
        use turbo_ferret::*;

        let before = system::get();

        let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
        ui.services_mut().log = None;
        ui.services_mut().clipboard = Box::new(MiniquadClipboard::new());
        ui.set_viewport(Size::new(400, 300));
        let root = ui.root();
        let e = ui.add(root, EditField::default());
        ui.set_rect(e, Rect::new(10, 10, 300, 30));
        ui.update();
        ui.set_focus(e);

        let text = "héllo ✓ 😀";
        ui.set_text(e, text);
        let shortcut = |ui: &mut Ui, name: &str| ui.shortcut(TbId::new(name), Modifiers::NONE);
        assert!(shortcut(&mut ui, "selectall"));
        assert!(shortcut(&mut ui, "cut"));
        assert_eq!(ui.text(e), Some(""));
        // It is on the system clipboard, not just in our memory.
        assert_eq!(system::get(), SystemText::Text(text.to_string()));
        assert!(shortcut(&mut ui, "paste"));
        assert!(shortcut(&mut ui, "paste"));
        assert_eq!(ui.text(e).map(str::to_string), Some(format!("{text}{text}")));

        // Text copied by another app is what gets pasted.
        system::set("from elsewhere");
        shortcut(&mut ui, "selectall");
        shortcut(&mut ui, "paste");
        assert_eq!(ui.text(e), Some("from elsewhere"));

        if let SystemText::Text(t) = before {
            system::set(&t);
        }
    }
}
