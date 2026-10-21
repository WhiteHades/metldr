use crate::Result;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    net::{IpAddr, SocketAddr},
    path::{Component, Path, PathBuf},
};

pub struct McpServer {
    name: String,
    address: SocketAddr,
    token: String,
    directory: PathBuf,
}

impl McpServer {
    pub fn http(
        name: String,
        address: SocketAddr,
        token: String,
        directory: PathBuf,
    ) -> Result<Self> {
        let invalid = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(invalid(
                "MCP server name must be 1 to 64 ASCII letters, digits, or underscores",
            )
            .into());
        }
        if !matches!(address.ip(), IpAddr::V4(ip) if ip.is_loopback()) || address.port() == 0 {
            return Err(
                invalid("MCP server address must be IPv4 loopback with a nonzero port").into(),
            );
        }
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(
                invalid("MCP server token must contain exactly 64 ASCII hex characters").into(),
            );
        }
        if !directory.is_absolute()
            || directory.to_string_lossy().len() > 4096
            || directory
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(invalid("MCP registration directory must be absolute, at most 4096 bytes, and contain no parent traversal").into());
        }
        Ok(Self {
            name,
            address,
            token,
            directory,
        })
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }
}

pub(crate) struct Registration {
    path: PathBuf,
    directory: PathBuf,
    remove_directory: bool,
    remove_file: bool,
}

impl Registration {
    pub(crate) fn create(server: &McpServer) -> Result<Self> {
        let remove_directory = !server.directory.exists();
        if remove_directory {
            if let Some(parent) = server.directory.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            builder.create(&server.directory)?;
        } else if !server.directory.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "MCP registration path is not a directory",
            )
            .into());
        }
        let mut registration = Self {
            path: server.directory.join(format!("{}.mjs", server.name)),
            directory: server.directory.clone(),
            remove_directory,
            remove_file: false,
        };
        #[cfg(unix)]
        if !remove_directory {
            fs::set_permissions(&server.directory, fs::Permissions::from_mode(0o700))?;
        }

        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&registration.path)?;
        registration.remove_file = true;

        let name = serde_json::to_string(&server.name)?;
        let url = serde_json::to_string(&format!("http://{}/mcp", server.address))?;
        write!(
            file,
            "export default (pi) => pi.registerMcpServer({name}, {{ url: {url}, headers: {{ Authorization: 'Bearer ${{METLDR_PI_BROWSER_TOKEN}}', Connection: 'close' }}, exposure: 'direct', timeout: 30 }});\n"
        )?;
        file.flush()?;
        Ok(registration)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if self.remove_file {
            let _ = fs::remove_file(&self.path);
        }
        if self.remove_directory {
            let _ = fs::remove_dir(&self.directory);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[test]
    fn descriptor_registration_validates_and_cleans_only_owned_files() {
        let temp_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".tmp");
        fs::create_dir_all(&temp_root).unwrap();
        let directory = temp_root.join(format!("mcp-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let token = "ab".repeat(32);
        let server = McpServer::http(
            "browser_tools".into(),
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 43123).into(),
            token.clone(),
            directory.clone(),
        )
        .unwrap();
        assert!(
            McpServer::http(
                "invalid-name".into(),
                server.address,
                token.clone(),
                directory.clone(),
            )
            .is_err()
        );
        assert!(
            McpServer::http(
                "browser_tools".into(),
                server.address,
                "short".into(),
                directory.clone(),
            )
            .is_err()
        );

        let path = directory.join("browser_tools.mjs");
        fs::write(&path, "preexisting").unwrap();
        assert!(Registration::create(&server).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "preexisting");
        fs::remove_file(&path).unwrap();

        let registration = Registration::create(&server).unwrap();
        let source = fs::read_to_string(registration.path()).unwrap();
        assert!(!source.contains(&token));
        assert!(source.contains("${METLDR_PI_BROWSER_TOKEN}"));
        assert!(source.contains("Connection: 'close'"));
        drop(registration);
        assert!(!path.exists());
        assert!(directory.is_dir());
        fs::remove_dir_all(directory).unwrap();
        let _ = fs::remove_dir(temp_root);
    }
}

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
