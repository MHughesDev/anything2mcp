# `a2m-netstate`

What this machine already knows about its own place on its networks, read from the operating system as an ordinary user, with nothing sent to any other machine. It is the same information a person sees in the system's network settings: the interfaces and their addresses, the routes and the default routes, the name servers and search domains, the neighbour table the system already holds, the sockets listening on this machine with their owners, and the proxy setting the system states. And a summary of where the machine stands: for each interface, the kind of place each of its addresses is. It is the first rung of the [network-awareness design](../../plan/network-awareness.md), the only one built ([decision 0243](../../decisions/0243-the-machine-reads-its-own-network-state-and-sends-nothing.md)).

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 2, Foundations |
| May depend on | `a2m-core` |
| Depends on | `a2m-core`, for `AddressScope` and `ScopedHost` (the kind of place an address's literal says it is, [decision 0240](../../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)) and `Failure`. Outside: `serde`; on Linux `rustix` with its `net` feature, for the route-netlink socket; on macOS `libc`, for `getifaddrs` and `sysctl`; on Windows `windows-sys`, for the IP Helper functions, the current user's proxy setting, and a process's program name. All four were already in the workspace's tree. The list is closed and a test holds it |
| Roadmap packages | None yet; built as the first slice of the network-awareness design (its slice N1), under [decision 0242](../../decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md) point 10 |
| Carries out | [Decision 0222](../../decisions/0222-the-product-is-aware-of-the-network-its-machine-is-on.md); [decision 0242](../../decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md) points 6 to 8 (nothing is sent on a network the person has not named; this crate sends nothing on any); [decision 0197](../../decisions/0197-no-client-takes-a-proxy-from-the-environment-and-stand-only-protocols.md) (a proxy is read to be shown, never applied); research notes [28](../../research/28-how-a-network-can-be-seen-from-one-machine.md), parts 1, 5, 6, 7, 8, and [29](../../research/29-network-tools-for-agents-and-the-conduct-of-looking.md) |

## Owns

- The seven readers: interfaces, routes, name servers, neighbours, listening sockets, proxy, and the standing that joins the first two.
- The typed facts they return, their serialized form, and the privacy choices that form applies.
- The bound on every list, and the record of what a bound cut.
- The system calls of each operating system that read these facts, and the closed list of them.

It must never: open a connection to another machine, send a packet onto a network, resolve a name, send an echo request, refresh a neighbour entry by asking, fetch a proxy script, or apply a proxy; change any table of the system; read another process's command line or environment; read the wireless network's name; decide anything (it reports; the policy engine and, later, the reach decide).

## Why it is a kernel crate at layer 2

The readers inspect the engine's own host, as `doctor.run` does, and the built-in actions that serve them live in `a2m-actions` at layer 7, which may not depend on a pack. The place derivation that comes next (slice N2 of the design) belongs in `a2m-provider`, at layer 5, beside the path check, and needs the machine's own subnets; layer 2 serves both. It stands alone, as the other foundations do: it needs only the shared types. `a2m-vault` already reads per-system key stores at this layer, so per-system code here has a precedent. What it knows is the machine's own network stack, which every pack that reaches a network shares; knowledge of one kind of system reached over a network stays in that system's pack (rule 8). The rung-1 to rung-3 looking of the design (announcements, one named host) is still planned for a pack, a2m-pack-net; it would read this crate's facts, and send.

## Interface

```rust
// Readers. Each blocks on system calls; an async caller runs it on a blocking thread.
pub fn interfaces() -> Result<Interfaces, NetError>;   // by index; each interface's addresses in order
pub fn routes() -> Result<Routes, NetError>;           // ordered; default routes per family, lowest metric first
pub fn resolvers() -> Result<Resolvers, NetError>;     // each place a setting is stated, in order read
pub fn neighbours() -> Result<Neighbours, NetError>;   // the table as held; never refreshed
pub fn listeners() -> Result<Listeners, NetError>;     // TCP listening, UDP bound with no peer; owner where visible
pub fn proxy() -> Result<Proxy, NetError>;             // as the system states it; credentials removed first;
                                                       // a list holding `@` left out whole (may_hold_credentials)
pub fn standing() -> Result<Standing, NetError>;       // interfaces() and routes(), joined
pub fn standing_of(&Interfaces, &Routes) -> Standing;  // the join, pure

// Facts. A field the system does not state is None or empty, never guessed.
pub struct Interface { index: u32, name, kind: Option<InterfaceKind>, system_kind: Option<String>,
    state: Option<LinkState>, mtu: Option<u32>, hardware_address: Option<HardwareAddress>,
    addresses: Vec<InterfaceAddress>, absent: Vec<Absent> }
pub enum InterfaceKind { Loopback, Wired, Wireless, Virtual, Tunnel, Other }
pub struct Route { family, destination: IpAddr, prefix_length: u8, gateway: Option<IpAddr>,
    interface_index: Option<u32>, interface: Option<String>, metric: Option<u32> }
pub struct ResolverSetting { from: String, name_servers: Vec<IpAddr>, search_domains: Vec<String> }
pub struct Neighbour { address, interface_index, interface, hardware_address, state: Option<NeighbourState> }
pub struct Listener { transport: Transport, address, port, scope: AddressScope,
    owner: Option<Owner>, owner_absent: Option<AbsentReason> }
pub struct ProxySetting { from, applies_to, value, credentials_removed: bool }
pub struct Standing { interfaces: Vec<InterfaceStanding>, default_routes: Vec<Route>, cut }
pub struct HardwareAddress(..); // octets() for the kernel; serialized as length and locally_administered only
pub struct Cut { list, kept, seen, table_cut: bool }
pub struct Absent { field, reason: AbsentReason }
pub enum AbsentReason { NotStated, NeedsLocationPermission, NotVisibleToThisUser, NotRead, MayHoldCredentials }
pub enum NetError { Refused { fact }, NotBuilt { fact, system }, Unreadable { fact, what }, Failed { fact, code } }
impl From<NetError> for Failure; // the shared failure of the core crate

// Pure text readers, public for the tests of other systems: resolv.conf, the kernel's
// /proc tables, a network mask, and a proxy value with its credentials removed.
pub mod text;
pub mod bound; // the bounds, as constants
```

Each reading carries `read_from`, the names of the system interfaces it read, so a caller sees the basis of every answer.

## What each system is asked, and nothing else

| Fact | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Interfaces, addresses | Route-netlink dumps `RTM_GETLINK`, `RTM_GETADDR`; `/sys/class/net/<name>/{ifindex,wireless,phy80211,device}` for wired or wireless, only when `/sys` names the same index | `getifaddrs`, `freeifaddrs` | `GetAdaptersAddresses` |
| Routes | Route-netlink `RTM_GETROUTE`: the main table's unicast routes | `sysctl` `NET_RT_DUMP`: routes not scoped to one interface | `GetIpForwardTable2`, `FreeMibTable`; interface metrics from `GetAdaptersAddresses` |
| Name servers | `/etc/resolv.conf`; `/run/systemd/resolve/resolv.conf` where it exists | `/etc/resolv.conf` | `GetAdaptersAddresses` |
| Neighbours | Route-netlink `RTM_GETNEIGH` | `sysctl` `NET_RT_FLAGS` with `RTF_LLINFO` | `GetIpNetTable2`, `FreeMibTable` |
| Listening sockets | `/proc/net/{tcp,tcp6,udp,udp6}`; `/proc/<pid>/fd` links; the file name of the `/proc/<pid>/exe` link. The walk is bounded by `MOST_PROCESSES` and `MOST_DESCRIPTORS`; an owner it did not reach is absent as `not_read`, one it could not see as `not_visible_to_this_user` | Not built | `GetExtendedTcpTable`, `GetExtendedUdpTable`; `OpenProcess` (limited query right), `QueryFullProcessImageNameW`, `CloseHandle` |
| Proxy | This process's environment, by the common variable names | Not built | `WinHttpGetIEProxyConfigForCurrentUser`, `GlobalFree` |

The one message sent anywhere is on Linux: a route-netlink request, addressed to the kernel (port 0, no multicast groups), of one of four `GET` types. A neighbour dump lists the cache; it makes the kernel ask nothing. The answer is read with `recvfrom`: a datagram from any sender but the kernel (port 0) is dropped unread, since another process in the namespace may address the socket's port, and each receive waits at most five seconds, after which the read is `net_read_failed`.

## Privacy

A hardware address, a host name, and a wireless network's name are personal data: they identify a device or a place across networks (research note 29).

| Field | Returned by default | What an action that serves it does |
| --- | --- | --- |
| Hardware address of an interface or a neighbour | Left out of the serialized form; it shows the length and whether it is locally administered (a randomized address is). The typed value keeps the octets for the kernel's own later use, a network's fingerprint | Shows the serialized form only. No input reveals the octets. A later need to tell two devices apart takes a keyed digest, not the octets |
| Host names | None is read: no reverse lookup, no hosts file, not the machine's own name | Nothing to serve |
| Wireless network's name | Not read. On a wireless interface it is listed as absent: on Windows the reason is `needs_location_permission`, elsewhere `not_read` | Nothing to serve; asking for the permission is a later slice and the person's act |
| Interface names, addresses, routes, name servers, search domains | Whole | Whole. They are what the machine's own settings show the person, and an agent on this machine can read them already |
| A listening socket's owner | Its process id, and its program's file name, without a folder or arguments: on Linux the file name of the `exe` link, which the process cannot set (its `comm` it can). Never a command line or an environment, which can hold secrets (rule 4) | Whole |
| Proxy setting | Whole, with any user name and password removed before any other cut (everything up to the last `@` after a proxy scheme), then the query and fragment; `credentials_removed` says when. A list of entries (the Windows proxy list) or a bypass list (the Windows bypass, `no_proxy`) that holds an `@` anywhere is left out whole, with an absent entry whose reason is `may_hold_credentials`: it is split on characters a password may hold, so no entry can be cut from it safely | Whole, as returned |

## Used by, and uses

- **Used by:** `a2m-actions`, for the built-in read actions (not wired yet; another agent owns that crate). Later, `a2m-provider` for the place of a destination (slice N2), and the network pack.
- **Uses:** `a2m-core` (`AddressScope`, `ScopedHost`, `Failure`).

## State

None. Every reading is made when asked and kept by no one here.

## Failures

| Code | Class | When |
| --- | --- | --- |
| `net_fact_refused` | `edge_blocked` | The system refused the fact to this user |
| `net_reader_not_built` | `unknown_variant` | No reader of the fact is built for this system |
| `net_answer_unreadable` | `unknown_variant` | The system answered in a shape the reader does not know |
| `net_read_failed` | `transient` | A system call failed for another reason |

A list a bound cut is not a failure: the reading names it in `cut`.

## Invariants

| Invariant | Test |
| --- | --- |
| No source file connects, sends, listens, binds, or resolves; none opens a socket but the one netlink socket, writes to one, or calls `ioctl` or `WSAIoctl`; none starts a process; none calls a Windows function that sends or changes a table; none reads a command line or an environment. Every line but the final unit-test module is scanned | `tests/sends_nothing.rs`, `no_source_file_connects_sends_listens_or_resolves`, `the_scan_leaves_out_only_the_unit_test_module` |
| The one send is to the kernel, in the netlink module, of the four listing requests only | `tests/sends_nothing.rs`, `the_one_send_goes_to_this_machines_kernel`; `netlink.rs`, `only_the_four_listing_requests_are_sent` |
| Only the kernel's answer is read, and a receive waits a bounded time (Linux) | `netlink.rs`, `a_message_from_another_process_is_dropped_unread`, `a_kernel_that_never_answers_is_a_read_failure_not_a_wait_for_good` |
| The dependencies are a closed list | `tests/sends_nothing.rs`, `the_dependencies_are_a_closed_list` |
| Every reader, in a network namespace with only loopback, gives that namespace's facts (Linux). This proves the readers read their own namespace; it does not alone prove that nothing is sent, since the namespace has nowhere to send to | `tests/namespace.rs` |
| Each Linux reader agrees with the system by another route: `/sys/class/net`, `/proc/net/fib_trie`, `/proc/net/if_inet6`, `/proc/net/route`, `/proc/net/arp`, `/etc/resolv.conf`, and a socket the test opened | `tests/linux.rs` |
| On Windows and macOS: loopback is listed and is this machine; each default route's interface is listed; a socket the test opened is listed with this process as owner (Windows); the IPv4 neighbours include what `arp` lists; on macOS the interfaces include what `ifconfig -l` lists and the IPv4 default gateway is one `netstat -rn` lists | `tests/windows_and_macos.rs`, first run in CI |
| A hardware address is never serialized whole; a proxy's credentials never survive, whatever the password holds; a list that may hold them is left out whole; a value without them is kept as written | `lib.rs`, `a_hardware_address_is_never_serialized_whole`; `text.rs`, `credentials_never_survive_in_a_proxy_value`, `no_byte_of_a_proxy_password_survives_cleaning_whatever_it_holds`, `a_list_that_holds_credentials_is_left_out_whole`, `a_value_with_no_credentials_is_kept_as_it_was` |
| A long list is cut and the cut is named; the default routes are bounded too; an owner walk a bound stopped says `not_read` | `bound.rs`; `lib.rs`, `the_default_routes_are_bounded_and_their_cut_is_named`; `linux.rs`, `a_walk_a_bound_stopped_says_the_owner_was_not_read` |
| Each error maps to one class and code | `error.rs` |

## Build order

1. Done: the types, the bounds, the error, and the pure text readers.
2. Done: the Linux readers, all seven, through route-netlink, `/proc`, and `/sys`, with the comparison tests and the namespace test.
3. Done in part: the Windows readers, all seven, through the IP Helper functions and the proxy setting. Compiled and linted for Windows; their tests run first in CI.
4. Done in part: the macOS readers of interfaces, routes, name servers, and neighbours. Compiled and linted for macOS; their tests run first in CI.
5. Not built: the macOS listening sockets with their owners (`proc_pidfdinfo`, whose socket record `libc` does not carry), and the macOS proxy setting (the system configuration's proxy dictionary). Each answers `net_reader_not_built`.
6. Not built: the wireless network's name, on any system.
7. Not built: the network's fingerprint (slice N3 of the design), which this crate's facts feed.
8. Done: the second reader's review of the crate ([decision 0261](../../decisions/0261-a-proxy-values-credentials-are-removed-before-any-other-cut.md)): credentials removed before any other cut, a list holding `@` left out whole, the default routes bounded, the owner walk bounded and its program named by the `exe` link, the netlink answer read from the kernel alone with a time limit, and the source scan widened. Not done from that review: on Windows the UDP table lists connected endpoints too, and on macOS an Ethernet-framed interface has no absent entry for its kind (finding A5); the corpus of address spellings (A7), which belongs to `a2m-core`.

## Open points

- On Linux, the name servers are read from the files; the per-link servers systemd-resolved holds on the system bus are not.
- On Linux, the proxy is read from this process's environment by convention. A desktop's proxy setting is not read.
- On Linux, routes are the main table's unicast routes; policy-routing tables and multipath next hops are not read (a multipath route's gateway is absent).
- On macOS, an Ethernet-framed interface is wired or wireless and the link-layer type does not say which: its kind is absent.
- On Windows, a route's metric is the route's own plus its interface's, which is what the system adds to choose; the interface metric comes from the adapter record.
- The readers were run as root here; that an ordinary user sees the same, and sees the owners of only its own sockets, is stated by the systems' documents (note 28) and not yet proven by a test run as another user.
