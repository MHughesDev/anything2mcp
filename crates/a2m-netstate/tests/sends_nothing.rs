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
    // A socket of any kind other than the one netlink socket, which is
    // opened with `socket_with`, and a raw write or control call on one.
    "socket(",
    "write(",
    "ioctl",
    "WSAIoctl",
    // A process started to read a fact for the reader: a command such as
    // `arp`, `netsh`, or `scutil` may send, and its output is not typed.
    "Command::new",
    "std::process",
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
    for entry in std::fs::read_dir(folder)
        .unwrap_or_else(|e| panic!("{e}"))
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// A file's code: comment lines dropped, and the unit-test module left out.
/// Only a module written as `#[cfg(test)]` and then `mod tests {` at the
/// start of a line is left out, up to the first line that is `}` alone, so
/// any code after it, or after any other test-only item, is still scanned.
fn code_of(text: &str) -> String {
    let mut kept = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line == "#[cfg(test)]" {
            let mut next = lines.clone();
            if next.next() == Some("mod tests {") {
                lines = next;
                for inner in lines.by_ref() {
                    if inner == "}" {
                        break;
                    }
                }
                continue;
            }
        }
        if !line.trim_start().starts_with("//") {
            kept.push(line);
        }
    }
    kept.join("\n")
}

fn code(path: &Path) -> String {
    code_of(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{e}")))
}

#[test]
fn the_scan_leaves_out_only_the_unit_test_module() {
    let text = "use a;\n#[cfg(test)]\nfn early() {}\nTcpStream\n// UdpSocket in a comment\n#[cfg(test)]\nmod tests {\n    fn f() {\n        lookup_host\n    }\n}\nconnect(\n";
    let code = code_of(text);
    assert!(code.contains("TcpStream"), "{code}");
    assert!(code.contains("fn early"), "{code}");
    assert!(code.contains("connect("), "{code}");
    assert!(!code.contains("UdpSocket"), "{code}");
    assert!(!code.contains("lookup_host"), "{code}");
}

fn all_sources() -> Vec<PathBuf> {
    let mut found = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    found.sort();
    found
}

#[test]
fn no_source_file_connects_sends_listens_or_resolves() {
    let files = all_sources();
    // The readers of all three systems are here.
    for name in [
        "linux.rs",
        "netlink.rs",
        "macos.rs",
        "windows.rs",
        "other.rs",
    ] {
        assert!(files.iter().any(|f| f.ends_with(name)), "{name} is missing");
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
        let name = line.split(['=', '.']).next().unwrap_or_default().trim();
        assert!(DEPENDENCIES.contains(&name), "`{name}` is not on the list");
    }
    // rustix's only added feature is its socket calls, for netlink.
    assert!(manifest.contains("rustix = { workspace = true, features = [\"net\"] }"));
}
