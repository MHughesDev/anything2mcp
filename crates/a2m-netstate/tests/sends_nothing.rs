//! The crate sends nothing to another machine, held by its source: no call
//! that connects, sends, listens, or resolves appears in it, the one send
//! there is goes to this machine's kernel, and its dependencies are a closed
//! list. Runs on every system; it reads the source files, so it holds for
//! the code of every system at once.

use std::path::{Path, PathBuf};

/// Names that connect, send, listen, resolve, or make the system ask
/// another machine something. None may appear in the crate's code.
const FORBIDDEN: &[&str] = &[
    // Sockets and clients of the standard library and of Tokio.
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "ToSocketAddrs",
    "lookup_host",
    "tokio::net",
    "std::net::Tcp",
    "std::net::Udp",
    // Any connect, listen, or bind, in any library's spelling.
    "connect(",
    "listen(",
    "bind(",
    "accept(",
    // Sends other than the one to the kernel, below.
    "send(",
    "send_to(",
    "sendmsg",
    "sendmmsg",
    "WSASend",
    "WSAConnect",
    // Name resolution, of every system.
    "getaddrinfo",
    "GetAddrInfo",
    "gethostbyname",
    "gethostbyaddr",
    "getnameinfo",
    "GetNameInfo",
    "DnsQuery",
    "res_query",
    "res_search",
    "DNSServiceQuery",
    "CFHost",
    // Windows calls that send: an address-resolution request, an echo, a
    // neighbour refresh, a proxy script fetch, an HTTP request.
    "SendARP",
    "ResolveIpNetEntry2",
    "ResolveNeighbor",
    "IcmpSendEcho",
    "Icmp6SendEcho",
    "WinHttpGetProxyForUrl",
    "WinHttpDetectAutoProxyConfigUrl",
    "WinHttpOpen",
    "WinHttpSendRequest",
    "WinHttpConnect",
    // Windows calls that change a table.
    "CreateIpNetEntry",
    "SetIpNetEntry",
    "DeleteIpNetEntry",
    "FlushIpNetTable",
    "CreateIpForwardEntry",
    "SetIpForwardEntry",
    "DeleteIpForwardEntry",
    "SetTcpEntry",
    // HTTP and DNS libraries.
    "reqwest",
    "hyper",
    "hickory",
    "trust_dns",
    // A command line or an environment of another process can hold a
    // secret, and is never read.
    "cmdline",
    "/environ",
    "KERN_PROCARGS",
    "PEB",
];

/// The only dependencies the crate may have.
const DEPENDENCIES: &[&str] = &["a2m-core", "serde", "rustix", "libc", "windows-sys"];

fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// A file's code: comment lines dropped, and the test module cut off.
fn code(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap();
    let body = text.split("#[cfg(test)]").next().unwrap_or_default();
    body.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn all_sources() -> Vec<PathBuf> {
    let mut found = Vec::new();
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut found);
    found.sort();
    found
}

#[test]
fn no_source_file_connects_sends_listens_or_resolves() {
    let files = all_sources();
    // The readers of all three systems are here.
    for name in ["linux.rs", "netlink.rs", "macos.rs", "windows.rs", "other.rs"] {
        assert!(
            files.iter().any(|f| f.ends_with(name)),
            "{name} is missing"
        );
    }
    for file in &files {
        let code = code(file);
        for word in FORBIDDEN {
            assert!(
                !code.contains(word),
                "`{word}` appears in {}",
                file.display()
            );
        }
    }
}

#[test]
fn the_one_send_goes_to_this_machines_kernel() {
    let mut sends = Vec::new();
    for file in all_sources() {
        let code = code(&file);
        for (at, _) in code.match_indices("sendto(") {
            sends.push((file.clone(), at));
        }
        if file.ends_with("netlink.rs") {
            // Addressed to the kernel: port 0, no multicast groups, and only
            // the four listing requests.
            assert!(code.contains("SocketAddrNetlink::new(0, 0)"));
            assert!(
                code.contains(
                    "pub(crate) const REQUESTS: &[u16] = &[RTM_GETLINK, RTM_GETADDR, RTM_GETROUTE, RTM_GETNEIGH];"
                )
            );
            assert!(code.contains(".filter(|_| REQUESTS.contains(&request))"));
        }
    }
    assert_eq!(sends.len(), 1, "{sends:?}");
    assert!(sends[0].0.ends_with("netlink.rs"));
}

#[test]
fn the_dependencies_are_a_closed_list() {
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let mut section = String::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.to_owned();
            continue;
        }
        let is_dependency = section.ends_with("dependencies]") && !section.contains("dev-");
        if !is_dependency || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let name = line
            .split(['=', '.'])
            .next()
            .unwrap_or_default()
            .trim();
        assert!(DEPENDENCIES.contains(&name), "`{name}` is not on the list");
    }
    // rustix's only added feature is its socket calls, for netlink.
    assert!(manifest.contains("rustix = { workspace = true, features = [\"net\"] }"));
}
