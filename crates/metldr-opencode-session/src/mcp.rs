use crate::{Result, invalid};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf};

pub enum McpServer {
    Http {
        name: String,
        url: String,
        headers: Vec<(String, String)>,
    },
    Stdio {
        name: String,
        command: PathBuf,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
}

pub(crate) fn declarations(servers: &[McpServer]) -> Result<Value> {
    if servers.len() > 16 {
        return Err(invalid("too many OpenCode MCP servers"));
    }
    let mut names = BTreeSet::new();
    let mut result = Vec::with_capacity(servers.len());
    for server in servers {
        let name = match server {
            McpServer::Http { name, .. } | McpServer::Stdio { name, .. } => name,
        };
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || !names.insert(name)
        {
            return Err(invalid("invalid or duplicate OpenCode MCP server name"));
        }
        result.push(match server {
            McpServer::Http { url, headers, .. } => {
                if !(url.starts_with("http://") || url.starts_with("https://"))
                    || url.len() > 2048
                    || url.bytes().any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
                    || url
                        .split_once("://")
                        .is_none_or(|(_, rest)| rest.split(['/', '?', '#']).next().is_none_or(str::is_empty))
                    || headers.len() > 32
                {
                    return Err(invalid("invalid OpenCode HTTP MCP configuration"));
                }
                let mut keys = BTreeSet::new();
                for (key, value) in headers {
                    if key.is_empty()
                        || key.len() > 128
                        || !key.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || b"!#$%&'*+-.^_`|~".contains(&byte)
                        })
                        || value.len() > 8192
                        || value.bytes().any(|byte| byte.is_ascii_control())
                        || !keys.insert(key.to_ascii_lowercase())
                    {
                        return Err(invalid("invalid OpenCode HTTP MCP headers"));
                    }
                }
                json!({"type":"http", "name": name, "url": url, "headers": headers.iter().map(|(name, value)| json!({"name": name, "value": value})).collect::<Vec<_>>()})
            }
            McpServer::Stdio { command, args, env, .. } => {
                let command = command
                    .to_str()
                    .filter(|path| command.is_absolute() && path.len() <= 2048 && !path.contains('\0'))
                    .ok_or_else(|| invalid("invalid OpenCode stdio MCP command"))?;
                if args.len() > 64
                    || args.iter().any(|arg| arg.len() > 4096 || arg.contains('\0'))
                    || env.len() > 32
                {
                    return Err(invalid("invalid OpenCode stdio MCP arguments or environment"));
                }
                let mut keys = BTreeSet::new();
                for (key, value) in env {
                    if key.is_empty()
                        || key.len() > 128
                        || !key.bytes().enumerate().all(|(index, byte)| {
                            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
                        })
                        || value.len() > 8192
                        || value.contains('\0')
                        || !keys.insert(key)
                    {
                        return Err(invalid("invalid OpenCode stdio MCP environment"));
                    }
                }
                json!({"name": name, "command": command, "args": args, "env": env.iter().map(|(name, value)| json!({"name": name, "value": value})).collect::<Vec<_>>()})
            }
        });
    }
    let declarations = Value::Array(result);
    if serde_json::to_vec(&declarations)?.len() > 128 * 1024 {
        return Err(invalid("OpenCode MCP declarations exceed size limit"));
    }
    Ok(declarations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_validate_and_encode_native_server_configs() {
        let executable = std::env::current_exe().unwrap();
        let servers = [
            McpServer::Http {
                name: "web_search".into(),
                url: "https://example.test/mcp".into(),
                headers: vec![("Authorization".into(), "secret".into())],
            },
            McpServer::Stdio {
                name: "local".into(),
                command: executable.clone(),
                args: vec!["--serve".into()],
                env: vec![("TOKEN".into(), "secret".into())],
            },
        ];
        assert_eq!(
            declarations(&servers).unwrap(),
            json!([
                {"type":"http","name":"web_search","url":"https://example.test/mcp","headers":[{"name":"Authorization","value":"secret"}]},
                {"name":"local","command":executable,"args":["--serve"],"env":[{"name":"TOKEN","value":"secret"}]}
            ])
        );
        assert!(
            declarations(&[McpServer::Stdio {
                name: "local".into(),
                command: "relative".into(),
                args: vec![],
                env: vec![],
            }])
            .is_err()
        );
        assert!(
            declarations(&[McpServer::Http {
                name: "web".into(),
                url: "https://example.test".into(),
                headers: vec![
                    ("X-Test".into(), "ok".into()),
                    ("x-test".into(), "no".into())
                ],
            }])
            .is_err()
        );
        assert!(
            declarations(&[
                McpServer::Stdio {
                    name: "same".into(),
                    command: executable.clone(),
                    args: vec![],
                    env: vec![],
                },
                McpServer::Stdio {
                    name: "same".into(),
                    command: executable,
                    args: vec![],
                    env: vec![],
                },
            ])
            .is_err()
        );
        assert!(
            declarations(&[McpServer::Http {
                name: "web".into(),
                url: "https://example.test".into(),
                headers: vec![("X-Test".into(), "ok\r\ninjected: yes".into())],
            }])
            .is_err()
        );
    }
}
