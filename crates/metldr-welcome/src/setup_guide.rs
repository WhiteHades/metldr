#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionOrigin(String);

impl ExtensionOrigin {
    pub fn from_id(id: &str) -> Option<Self> {
        (id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte)))
            .then(|| Self(format!("chrome-extension://{id}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Origin,
    LinuxEdit,
    LinuxOverride,
    LinuxRestart,
    MacOrigin,
    PosixServe,
    WindowsServe,
    ListModels,
    PullModel,
    ExtensionsPage,
}

pub const COMMANDS: [Command; 10] = [
    Command::Origin,
    Command::LinuxEdit,
    Command::LinuxOverride,
    Command::LinuxRestart,
    Command::MacOrigin,
    Command::PosixServe,
    Command::WindowsServe,
    Command::ListModels,
    Command::PullModel,
    Command::ExtensionsPage,
];

impl Command {
    pub fn id(self) -> &'static str {
        match self {
            Self::Origin => "origin",
            Self::LinuxEdit => "linux-edit",
            Self::LinuxOverride => "linux-override",
            Self::LinuxRestart => "linux-restart",
            Self::MacOrigin => "mac-origin",
            Self::PosixServe => "posix-serve",
            Self::WindowsServe => "windows-serve",
            Self::ListModels => "list-models",
            Self::PullModel => "pull-model",
            Self::ExtensionsPage => "extensions-page",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Origin => "Extension origin",
            Self::LinuxEdit => "Edit Ollama service",
            Self::LinuxOverride => "Origin setting",
            Self::LinuxRestart => "Reload and restart service",
            Self::MacOrigin => "Set origin for Ollama app",
            Self::PosixServe => "Start temporary server",
            Self::WindowsServe => "Start temporary server",
            Self::ListModels => "List models",
            Self::PullModel => "Download a model",
            Self::ExtensionsPage => "Open extensions page",
        }
    }

    pub fn text(self, origin: Option<&ExtensionOrigin>) -> String {
        let origin = origin
            .map(ExtensionOrigin::as_str)
            .unwrap_or("chrome-extension://YOUR_EXTENSION_ID");
        match self {
            Self::Origin => origin.into(),
            Self::LinuxEdit => "sudo systemctl edit ollama.service".into(),
            Self::LinuxOverride => format!("[Service]\nEnvironment=\"OLLAMA_ORIGINS={origin}\""),
            Self::LinuxRestart => {
                "sudo systemctl daemon-reload\nsudo systemctl restart ollama".into()
            }
            Self::MacOrigin => format!("launchctl setenv OLLAMA_ORIGINS \"{origin}\""),
            Self::PosixServe => format!("OLLAMA_ORIGINS=\"{origin}\" ollama serve"),
            Self::WindowsServe => format!("$env:OLLAMA_ORIGINS=\"{origin}\"\nollama serve"),
            Self::ListModels => "ollama ls".into(),
            Self::PullModel => "ollama pull MODEL_NAME".into(),
            Self::ExtensionsPage => "chrome://extensions".into(),
        }
    }
}

pub fn render_html() -> String {
    let mut html = String::from(
        "<details id=\"ollama-setup\" class=\"setup-guide\"><summary>Set up Ollama</summary>\
        <p><a href=\"https://ollama.com/download\" target=\"_blank\" rel=\"noreferrer\">Install Ollama</a> from its official download page. The extension checks the model list at 127.0.0.1:11434. Ollama supports local and cloud models. A cloud model can send content to its provider.</p>\
        <h4>Allow this extension</h4><p>The origin below updates to match this extension when setup loads. If it stays a placeholder, find your extension ID at chrome://extensions. Enable Developer mode to see it, then replace YOUR_EXTENSION_ID manually.</p>",
    );
    html.push_str(&command_html(Command::Origin));
    html.push_str("<p id=\"setup-copy-status\" role=\"status\">Copy unavailable until setup loads. Commands remain selectable.</p>\
        <p>Keep any existing OLLAMA_ORIGINS entries and add this origin, separated by a comma. Allow only the origins you need. Keep the server bound to loopback instead of 0.0.0.0.</p>\
        <h4>Configure your existing app or service</h4><p>Finish current tasks before restarting Ollama.</p>\
        <details name=\"ollama-platform\"><summary>Linux with systemd</summary><p>Open the service override editor. Add the origin setting below, preserving any origins you already allow. Save the override, then reload and restart the service.</p>");
    for command in [
        Command::LinuxEdit,
        Command::LinuxOverride,
        Command::LinuxRestart,
    ] {
        html.push_str(&command_html(command));
    }
    html.push_str("</details><details name=\"ollama-platform\"><summary>macOS app</summary><p>Set the app environment with the command below, then quit and reopen Ollama.</p>");
    html.push_str(&command_html(Command::MacOrigin));
    html.push_str("</details><details name=\"ollama-platform\"><summary>Windows app</summary><p>Quit Ollama from its tray menu. In Windows, open Edit environment variables for your account. Add or edit the user variable OLLAMA_ORIGINS and use the extension origin above as its value. Preserve any existing entries, separated by a comma. Save, then open Ollama again.</p></details>\
        <h4>Temporary server</h4><p>Use these commands only after stopping the existing app or service. Both use the same port, so a second server cannot start while the first is running. The setting below lasts for the server started from that terminal.</p>\
        <details name=\"ollama-platform\"><summary>Linux or macOS terminal</summary>");
    html.push_str(&command_html(Command::PosixServe));
    html.push_str(
        "</details><details name=\"ollama-platform\"><summary>Windows PowerShell</summary>",
    );
    html.push_str(&command_html(Command::WindowsServe));
    html.push_str("</details><h4>Choose a model</h4><p>List available models with the first command. To download one, choose a name from the <a href=\"https://ollama.com/library\" target=\"_blank\" rel=\"noreferrer\">Ollama model library</a> and replace MODEL_NAME in the second command. Review its size, hardware needs and local or cloud behavior before downloading.</p>");
    for command in [
        Command::ListModels,
        Command::PullModel,
        Command::ExtensionsPage,
    ] {
        html.push_str(&command_html(command));
    }
    html.push_str("<p>After configuring Ollama, return to setup and choose Ollama, then Check. Select a model before saving.</p>\
        <p>Read the <a href=\"https://docs.ollama.com/faq\" target=\"_blank\" rel=\"noreferrer\">Ollama setup FAQ</a> or the <a href=\"https://docs.ollama.com/cli\" target=\"_blank\" rel=\"noreferrer\">CLI documentation</a> for more details.</p></details>");
    html
}

fn command_html(command: Command) -> String {
    format!(
        "<div class=\"setup-command\"><p class=\"command-label\">{label}</p><pre><code id=\"setup-command-{id}\">{text}</code></pre><button type=\"button\" disabled data-copy-command=\"{id}\">Copy {label}</button></div>",
        label = command.label(),
        id = command.id(),
        text = escape(&command.text(None))
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::{Command, ExtensionOrigin};

    #[test]
    fn validates_extension_ids_and_renders_expected_templates() {
        let origin = ExtensionOrigin::from_id("abcdefghijklmnopabcdefghijklmnop").unwrap();
        assert_eq!(
            origin.as_str(),
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop"
        );
        for invalid in [
            "",
            "abcdefghijklmnopabcdefghijklmnopx",
            "ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP",
            "abcdefghijklmnopabcdefghijklmnop/",
        ] {
            assert!(ExtensionOrigin::from_id(invalid).is_none());
        }
        assert_eq!(
            Command::LinuxOverride.text(Some(&origin)),
            "[Service]\nEnvironment=\"OLLAMA_ORIGINS=chrome-extension://abcdefghijklmnopabcdefghijklmnop\""
        );
        assert_eq!(
            Command::WindowsServe.text(None),
            "$env:OLLAMA_ORIGINS=\"chrome-extension://YOUR_EXTENSION_ID\"\nollama serve"
        );
        assert_eq!(Command::PullModel.text(None), "ollama pull MODEL_NAME");
    }
}
