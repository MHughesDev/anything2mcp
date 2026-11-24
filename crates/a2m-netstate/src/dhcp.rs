//! The DHCP server each interface was leased its address by, read from the
//! lease files the system's own client keeps, as an ordinary user. Nothing
//! is sent: a lease file is read, and no request is made of any server.
//!
//! Only Linux has a reader. On Windows the adapter record carries the
//! server and on macOS the dynamic store does; neither reader is built, so
//! on those systems the fact is [`NetError::NotBuilt`] and a fingerprint
//! names the server as not read (rule 7). A lease file this user may not
//! read is a refusal, never a guess.

use std::net::IpAddr;

use serde::Serialize;

use crate::bound::Cut;
use crate::error::NetError;

/// The DHCP server of one interface.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct DhcpServer {
    /// The interface's index, where the lease file names it by index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_index: Option<u32>,
    /// The interface's name, where the lease file names it by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
    /// The server's address.
    pub server: IpAddr,
}

/// The DHCP servers found, and where they were read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DhcpServers {
    /// One entry for each interface a lease names a server for.
    pub servers: Vec<DhcpServer>,
    /// Where they were read.
    pub read_from: Vec<&'static str>,
    /// What a bound cut.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cut: Vec<Cut>,
}

/// The most lease files read from one folder.
#[cfg(target_os = "linux")]
const MOST_LEASE_FILES: usize = 256;

#[cfg(target_os = "linux")]
pub(crate) fn read() -> Result<DhcpServers, NetError> {
    use std::io::Read;
    use std::path::{Path, PathBuf};

    use crate::bound::MOST_TABLE_BYTES;
    use crate::error::Fact;
    use crate::text;

    fn file(path: &Path) -> Result<Option<String>, NetError> {
        let opened = match std::fs::File::open(path) {
            Ok(opened) => opened,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(NetError::from_io(Fact::DhcpServers, &error)),
        };
        let mut bytes = Vec::new();
        opened
            .take(MOST_TABLE_BYTES as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| NetError::from_io(Fact::DhcpServers, &error))?;
        Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
    }

    // The files of a folder, in name order, at most `MOST_LEASE_FILES`. A
    // folder this user may not list is skipped: another source may serve.
    fn files(folder: &str) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        paths.truncate(MOST_LEASE_FILES);
        paths
    }

    let mut servers = Vec::new();
    let mut read_from = Vec::new();
    // systemd-networkd: one file per interface index.
    for path in files("/run/systemd/netif/leases") {
        let Some(index) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if let Some(text) = file(&path)?
            && let Some(server) = text::dhcp_lease_server(&text)
        {
            servers.push(DhcpServer {
                interface_index: Some(index),
                interface: None,
                server,
            });
            read_from.push("/run/systemd/netif/leases");
        }
    }
    // dhclient: lease files that name the interface in each lease.
    let mut dhclient: Vec<PathBuf> = vec![PathBuf::from("/var/lib/dhcp/dhclient.leases")];
    dhclient.extend(
        files("/var/lib/dhcp")
            .into_iter()
            .chain(files("/var/lib/dhclient"))
            .filter(|p| p.extension().is_some_and(|e| e == "leases")),
    );
    dhclient.sort();
    dhclient.dedup();
    for path in dhclient {
        if let Some(text) = file(&path)? {
            for (interface, server) in text::dhclient_leases(&text) {
                servers.push(DhcpServer {
                    interface_index: None,
                    interface: Some(interface),
                    server,
                });
                read_from.push("dhclient lease files");
            }
        }
    }
    servers.sort();
    servers.dedup();
    read_from.sort_unstable();
    read_from.dedup();
    Ok(DhcpServers {
        servers,
        read_from,
        cut: Vec::new(),
    })
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn read() -> Result<DhcpServers, NetError> {
    Err(NetError::NotBuilt {
        fact: crate::error::Fact::DhcpServers,
        system: std::env::consts::OS,
    })
}
