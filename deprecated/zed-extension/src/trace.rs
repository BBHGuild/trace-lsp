use zed_extension_api::{self as zed, Result};

struct TraceExtension;

impl TraceExtension {
    fn find_server(&self, worktree: &zed::Worktree, env: &[(String, String)]) -> Result<String> {
        if let Some(path) = worktree.which("trace-lsp") {
            return Ok(path);
        }

        // Check ~/.cargo/bin/trace-lsp as a common local cargo install location
        let cargo_bin = env
            .iter()
            .find(|(key, _)| key == "HOME")
            .map(|(_, home)| format!("{home}/.cargo/bin/trace-lsp"));

        if let Some(path) = cargo_bin {
            return Ok(path);
        }

        Err("trace-lsp not found in PATH or ~/.cargo/bin. Please run `cargo install --path crates/trace-lsp` or configure `lsp.trace-lsp.binary.path` in Zed settings.".to_string())
    }
}

impl zed::Extension for TraceExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = zed::settings::LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|settings| settings.binary);

        let (path, args, extra_env) = match binary {
            Some(b) => (b.path, b.arguments, b.env.unwrap_or_default()),
            None => (None, None, Default::default()),
        };

        let mut env = worktree.shell_env();
        env.retain(|(key, _)| !extra_env.contains_key(key));
        env.extend(extra_env);

        let command = match path {
            Some(p) => p,
            None => self.find_server(worktree, &env)?,
        };

        Ok(zed::Command {
            command,
            args: args.unwrap_or_else(|| vec!["--stdio".to_string()]),
            env,
        })
    }
}

zed::register_extension!(TraceExtension);
