# Research: how a network can be seen from one machine

**Status:** completed on 2026-11-18 by reading sources and registries. Nothing was installed, built, or run; no packet was sent to any network. Partial in ways that matter, stated here and again in "What could not be verified". Most standards sites, the Microsoft documentation site, the Rust documentation site, the nmap site, and the web-standards and vendor blog sites did not resolve from this session. What was read instead: Apple's documentation data files; Microsoft's documentation as published in its public source repositories (`MicrosoftDocs/sdk-api`, `MicrosoftDocs/win32`); Linux manual pages and kernel documentation as held in public repositories; the tools' own repositories and manual pages; and the crates' registry records, repository files, and, for one crate, its published source. Every claim carries a tag:

- **[read]**: the page or file was opened and read in this session.
- **[search]**: taken from a web-search summary; the page itself was not opened. Treat as a lead.
- **[memory]**: stated from knowledge of the standard or system, not opened in this session. Treat as unverified.

Feeds the design owed by [decision 0222](../decisions/0222-the-product-is-aware-of-the-network-its-machine-is-on.md) (sections [106](../spec/1-product/106-what-it-latches-onto.md), [218](../spec/2-kernel/218-discovery-and-findings.md), [402](../spec/4-building-tools/402-the-scout-and-who-does-what.md), [502](../spec/5-security/502-defending-against-a-manipulated-agent.md)) and extends [decision 0208](../decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md). Extends [note 26](26-desktop-pack-before-it-is-built.md) in method. This note records facts and candidate rules; the design is another agent's.

## Question

A program running as an ordinary user on one Windows, macOS, or Linux machine wants to say truthfully what is on the networks that machine can reach, so that a developer can then build tools over what was found. What ways are there to find and identify what is there; what does each cost, need, and risk; what does each operating system ask of the program; which Rust crates exist; and how does a permission given for one network avoid following the machine to another? The product's rules shape the answer: it operates openly (no stealth, no evading a network's protections, no credential guessing), an unknown thing is a stop and never a guess, and nothing happens that the person did not choose.

## Answer in short

- **Most of what a program needs to know is already on the machine and costs no packet.** Interfaces, addresses, routes, the gateway, DNS servers and search domains, the neighbour cache, listening sockets with their owning process, and the proxy setting can each be read by an ordinary user on all three systems through a documented interface (part 1). Reading first is quieter, cheaper, and needs no network permission. The weak spots are the wireless network's name (Windows now gates it behind a location consent; macOS returns nothing when the interface has no network), the full DHCP lease (no single simple interface on any system), and the neighbour table (no maintained cross-platform crate).
- **Self-announcement is cheap and standard, and wholly untrusted.** Asking for mDNS/DNS-SD, SSDP, and WS-Discovery each takes one small multicast query to a fixed link-local group, and listening for the answers needs no elevation on any system. Anyone on the link can announce anything, and SSDP's own description of itself points the client at a URL the responder chose (part 2). A found URL must never widen where the product goes.
- **Link-layer discovery (LLDP and similar) is out of reach for an ordinary program.** It needs raw link-layer sockets (Linux `CAP_NET_RAW`; a packet driver or privileged device on the others) and the frames stop at the first switch. Router advertisements and DHCP are consumed by the operating system; the product should read what they produced, not listen for them.
- **Asking becomes probing by breadth, not by protocol.** A TCP connect to one port, a TLS handshake, a GET of `/`: each is what any client does. The same acts over many ports and hosts is a scan. Published norms draw the line at permission, rate, identification, and an opt-out; the tools' own documents say to get permission before scanning a network that is not yours (part 3).
- **Unprivileged ICMP echo works on all three systems, with one condition on Linux.** Windows has an echo API for ordinary users, macOS and Linux have datagram ICMP sockets, and Linux gates them by a group range that the kernel sets to "nobody" and a common init system sets to "everybody" (part 3).
- **macOS is the system that asks the person.** Since macOS 15 a program's outgoing connection to a local address, any multicast send or receive, and any Bonjour or `.local` lookup needs the person's Local Network permission; there is no entitlement to ask for it on macOS. Command-line tools run from a terminal, daemons, and root programs are exempt; a program started by another program is judged by the one that started it (part 5). This decides how the product must be started to behave the same everywhere.
- **Nothing on a network identifies it stably and unspoofably.** The gateway's hardware address, the DHCP server, the DNS suffix, the wireless name and access-point identifier, and each system's own network identifier are all assertable by someone on the network or set once by the person. A permission should be bound to a combination, and any change should read as a new network (part 6).
- **"Private address" is not "my network".** Ranges 10/8, 172.16/12, 192.168/16, 100.64/10, fe80::/10, and fc00::/7 are all used on public wireless networks, corporate networks, hotels, carriers, and cloud networks. Browsers have moved from asking the device to opt in (Private Network Access) to asking the person per site (Local Network Access) (parts 6 and 7).
- **What a thing says about reading versus changing is rare and uneven.** Only some formats say so at all (a protocol's operation list, MCP tool hints, ONVIF's access classes as reported). OpenAPI has no such flag beyond the HTTP method. UPnP actions, SNMP's SET, and port-mapping requests are all writes on the network's own equipment (part 4).
- **The crate picture is thin where it matters.** Good, current crates exist for interface listing, system DNS configuration, listening sockets, mDNS, SSDP/UPnP, unprivileged ping, SNMP, and printers. There is none that is maintained and cross-platform for the neighbour table, the wireless name, the DHCP lease, LLMNR, NetBIOS name service, or link-layer discovery; those are thin adapters over operating-system interfaces the workspace already uses (part 8).
- **One tension with decision 0208 needs a decision.** That record owes a rule test that no product code opens a listener on any address but loopback. A discovery client binds UDP sockets on non-loopback addresses to receive multicast and unicast replies. These are not services, but a test written as "no socket bound on a non-loopback address" would forbid them (see "What it means for the design").

## What was read, and what could not be reached

| Source | Reached | How |
| --- | --- | --- |
| Apple's technical note on local network privacy; the CoreWLAN and System Configuration pages | Yes | Apple's documentation data files (`developer.apple.com`) |
| Microsoft's IP Helper, WLAN, Network List Manager, ICMP, WinHTTP, and DNS-SD pages | Yes, as source text | The public `MicrosoftDocs/sdk-api` and `MicrosoftDocs/win32` repositories. `learn.microsoft.com` itself did not resolve. The repository text can differ from the live page |
| Linux manual pages | Yes, from a mirror | The `mkerrisk/man-pages` repository. `man7.org` did not resolve. The mirror may be older than the current pages |
| Linux kernel documentation, systemd documentation and defaults | Yes | The `torvalds/linux` and `systemd/systemd` repositories, main branch, 2026-11-18 |
| RFC texts and the IETF, W3C, IANA, UPnP, ONVIF, PWG, and Chrome sites | No | None resolved. Every statement about an RFC's content is [memory] or [search] unless a crate's or tool's own file quotes it, and each says so |
| Nmap, ZMap, masscan, naabu, httpx documents | Yes | Their repositories (the nmap manual page source, the ZMap README and wiki, the others' READMEs) |
| Private Network Access and Local Network Access | Yes | The two WICG repositories' README files and the Local Network Access explainer. The specification pages did not resolve |
| MCP discovery | Yes | The MCP repository's SEP-2127 file, the server-card extension repository's discovery document, and the draft schema file |
| Crate records | Yes | The crates.io JSON interface, 2026-11-18: current version, release date, licence, repository, downloads. The crate documentation site did not resolve |
| Crate READMEs and source | Yes | Repository raw files; the published source of one crate (`ping-async` 1.0.2) was unpacked and read, not built |
| Advisories (RustSec) | No | The advisory database archive was refused. Nothing in this note says a crate is free of advisories |
| GitHub repository pages and API | No | Refused. No last-commit dates, issue counts, or star counts; the release date comes from the registry |
| Web searches | Yes | Generic technical terms and protocol numbers only. Nothing from this repository was sent |

## 1. What the machine already knows, and how an unprivileged program reads it

Reading these sends nothing. The interface, whether it needs elevation, and the crate are given per item. "Elevation: no" means the source documents no requirement, or is silent and the claim is [memory]; a run on each system would prove it.

| Fact | Windows | macOS | Linux | Crate |
| --- | --- | --- | --- | --- |
| Interfaces, addresses, prefixes, hardware addresses, interface type | `GetAdaptersAddresses` returns, per adapter, addresses, hardware address, interface type, connection type, tunnel type, gateway, DNS servers, DNS suffix, and DHCPv4 server; the page warns it is slow and synchronous [read: S7, S8] | `getifaddrs` [memory] | `getifaddrs`, or the route-netlink address dump [memory] | `if-addrs`, `netdev`, `network-interface`, `local-ip-address` |
| Routing table and default gateway | `GetIpForwardTable2` [read: S6]; the gateway is also a member of the adapter record [read: S8] | A routing-socket dump through `sysctl` (the `arp` source uses the same mechanism for neighbours) [read: S4]; `netdev` reads System Configuration data and falls back to the SC interface [read: S54] | Route-netlink `RTM_GETROUTE`; the route-netlink manual page says the interface lets a program read and alter the kernel's routing tables, and the capability line it states is for modifying and for listening to multicast groups [read: S21, S22] | `netdev` (default gateway on by default, feature `gateway`) |
| DNS servers and search domains | `FirstDnsServerAddress`, `DnsSuffix`, `FirstDnsSuffix` in the adapter record [read: S8] | System Configuration (`SCDynamicStoreCopyValue` reads values from the dynamic store by key) [read: S3]; the resolver crate has a macOS module [read: S52] | `/etc/resolv.conf`, or, where systemd-resolved runs, its per-link state (the documentation says it also maintains per-link LLMNR and mDNS settings) [read: S26] | `hickory-resolver`, feature `system-config`, which pulls `resolv-conf`, `ipconfig`, and `system-configuration` per system [read: S52] |
| Neighbour table (ARP, IPv6 neighbour discovery cache) | `GetIpNetTable2`, IPv4, IPv6, or both [read: S5] | A `sysctl` dump of routing entries flagged as link-layer info; the `arp` source's listing path has no user-id check in the lines read [read: S4] | Route-netlink `RTM_GETNEIGH`, or `/proc/net/arp` [memory] | None maintained (part 8) |
| DHCP lease contents | Server address in the adapter record [read: S8]. Lease times and options: older `GetAdaptersInfo` and the registry [memory] | System Configuration dynamic-store entries per service, and the command-line tool that prints the last packet [memory]. `netdev` says it reads "DHCP hints" from System Configuration plist files [read: S54] | Per daemon: NetworkManager exposes it on its D-Bus interface; systemd-networkd and dhclient keep lease files [memory] | None for the whole lease |
| Name of the wireless network | `WlanQueryInterface` with the current-connection opcode [read: S10], but see the next paragraph | `CWInterface.ssid()` returns the SSID, or nothing if the interface is not in a network [read: S2]; whether it needs the location permission is not in the page [memory says yes since macOS 14: unverified] | nl80211, NetworkManager's D-Bus interface, or the wireless extensions [memory] | `objc2-core-wlan` (macOS), `windows` (Windows); `wifiscanner` 0.5.1 is from 2019 and shells out |
| VPN and virtual interfaces | Interface type, tunnel type, connection type in the adapter record [read: S8] | `utun` and similar names; type from System Configuration [memory] | Link kind from route-netlink (`IFLA_INFO_KIND`) [memory] | `netdev` reports an interface type |
| System proxy configuration | `WinHttpGetIEProxyConfigForCurrentUser`: per connection, and "should not be used in a service process that does not impersonate a logged-on user" [read: S15] | System Configuration proxies [memory] | No single place: environment, desktop settings, per-tool files [memory] | None found |
| Hosts file | `%SystemRoot%\System32\drivers\etc\hosts` [memory] | `/etc/hosts` [memory] | `/etc/hosts` [memory] | None needed |
| Listening sockets and the owning program | `GetExtendedTcpTable` with an owner-process table class (IPv4 and IPv6) [read: S9] | `proc_pidfdinfo` per process [read: S58] | `/proc/net/tcp` (documented as deprecated in favour of the socket-diagnostic netlink interface) [read: S25] plus a walk of `/proc/<pid>/fd`; reading another user's process entries is gated by a ptrace access check, and `hidepid` can hide them [read: S24] | `listeners` (all three), `netstat2` (all three) |

Findings on this table:

- **Elevation is rarely the obstacle; ownership and consent are.** The Linux process walk and the macOS per-process call see only the person's own processes without privilege [read for Linux: S24; memory for macOS]. So "which program owns this port" is complete for the person's own programs and partial for the rest.
- **The Windows wireless name is behind a consent.** Microsoft's page, dated 2025-05-30, says that unless the person has allowed the program precise location, `WlanQueryInterface` with the current-connection opcode (and three other wlanapi calls) returns access denied, that a one-time system prompt appears for a program running in the person's context, and that the calls show in the in-use location indicator. A separate call that reads the connected SSID is named as the alternative [read: S11]. Microsoft says the details could change before release.
- **The Windows network-profile category is readable and labelled by the person.** `NLM_NETWORK_CATEGORY` has public, private, and domain-authenticated values; the page warns never to infer open firewall ports from them, because the person can change the defaults [read: S12].
- **The neighbour cache is a cheaper source than any sweep.** It lists hosts this machine has already spoken to or seen announced. It is empty or stale for a host nobody talked to.
- **Reading the proxy to report it is not using it.** Decision 0208 says a proxy is never taken silently from the environment. Reading the setting in order to show the person is a different act from using it; the design should say so.

## 2. What announces itself, and how to listen

The rows give what the protocol sends, how trustworthy the answer is, the standard, and the crate. Wire details are marked by tag. The tools' own source confirms what is sent only for SSDP.

| Protocol | Passive or query? What is sent, to where | What a responder reveals | Trust | Standard | Crate |
| --- | --- | --- | --- | --- | --- |
| mDNS and DNS-SD | Devices announce on start and re-announce [memory]. In practice a browser sends a DNS query for a service type (a pointer-record query for a name such as `_ipp._tcp.local`) from UDP to the link-local multicast group 224.0.0.251 (IPv6 ff02::fb), port 5353 [search: the multicast Ethernet address 01:00:5E:00:00:FB, W5; the group and port are [memory]]. A client can ask for a unicast reply with a flag in the question class; `mdns-sd` documents that it does not yet implement unicast responses [read: S45]. The general service-type list is enumerated through a reserved name [memory] | Instance name, host name, port, a key-value text record, addresses [memory] | None: any host on the link can answer for any name; the standard has a rule that a querier accepts unicast replies only to a query that asked for one [search: W5, second-hand] | RFC 6762 and RFC 6763 (named by the `mdns-sd` README) [read: S45] | `mdns-sd` 0.22.0 (sync or async, no runtime, v4 and v6, client and responder) |
| SSDP (UPnP discovery) | Query: `M-SEARCH * HTTP/1.1` with `Host: 239.255.255.250:1900`, `Man: "ssdp:discover"`, an `MX` delay, and a search target; the client sets a multicast TTL [read: S46 source]. Devices also send periodic `NOTIFY` announcements [memory], so listening alone works but slowly | A location URL to an XML description, plus search-target and unique-name headers [search: W2] | None. A search result says "fetch this URL"; SSDP does not check that a client or a root device is on the network it claims [search: W2, an Irish national security centre page] | UPnP Device Architecture 2.0 (named by two crates' READMEs) [read: S46, S47] | `ssdp-client` 2.1.0, `rupnp` 3.0.0 |
| WS-Discovery | A SOAP `Probe` over UDP to 239.255.255.250, port 3702; a `Probe` must have a non-empty types element; a `ProbeMatches` reply is unicast to the sender and must arrive within 4 seconds or the Windows firewall may drop it [search: W3, quoting Microsoft's interoperability notes]. Devices also send `Hello`/`Bye` [memory]. If the reply has no transport addresses, a multicast `Resolve` is needed [search: W3] | Types (for example a video-transmitter type), scopes, and transport addresses for further requests [search: W3, memory] | None | OASIS WS-Discovery 1.1 [memory] | `oxvif` has WS-Discovery [read: S53]; others are camera- or printer-specific (part 8) |
| LLMNR | Name queries to 224.0.0.252, UDP and TCP port 5355 [search: W4]. RFC 4795 is Informational, not an IETF standard [search: W4]. systemd-resolved can answer and resolve per link [read: S26]. The CISA countermeasure page advises disabling it [search: W4] | Only an address for a name asked | Poisonable by design: any host can answer | RFC 4795 | `packet-dissector-llmnr` (parser only) |
| NetBIOS name service | UDP 137 name queries and broadcast; a node-status query to a known address returns the names it holds [memory; search confirms only the port, W4] | Machine and workgroup names | Poisonable | RFC 1001, 1002 [memory] | `netbios-parser` 0.2.0 (2021, parser only), `nbtscanner` 0.0.1 (2021) |
| Link-layer discovery (LLDP; vendor equivalents) | Passive: frames to a reserved group address `01:80:C2:00:00:0E`, EtherType 0x88CC [search: W13] that bridges do not forward [memory]. Receiving needs a raw packet socket: on Linux the packet-socket manual page requires `CAP_NET_RAW` [search: W13]; on Windows and macOS a packet-capture driver or privileged device [memory]. A program can instead ask a running `lldpd` over its control socket | The neighbouring switch or access point: name, port, capabilities, management address | Spoofable by anyone on the same segment | IEEE 802.1AB | `rlldpctl` 1.0.2 (talks to a running `lldpd`), `packet-dissector-lldp` 0.6.3 (parser), `rawether` 0.2.0 (raw I/O) |
| Router advertisements and DHCP | The router multicasts ICMPv6 advertisements; a DHCP client broadcasts from port 68 [memory]. The operating system consumes both | Prefixes, gateway, DNS servers (RDNSS), lease and options | Rogue routers and rogue DHCP servers exist | RFC 4861, RFC 2131 [memory] | `dhcproto` 0.15.0 parses and builds DHCP; a program should read the result from part 1 and run no DHCP client of its own |
| NAT port-mapping protocols | NAT-PMP: a request to the gateway's UDP port 5351 for its external address [search: W10]; PCP is its successor on the same ports [search: W10]. UPnP IGD: after SSDP, the gateway's description names a connection service whose `GetExternalIPAddress` action returns the public address [memory]. The same services also add and remove port mappings, which is a change | The public address; whether a mapping can be made | Gateway-only; but a client on the link can ask | RFC 6886, RFC 6887, UPnP IGD | `igd-next` 0.18.0 "can search for the gateway, request the external address, and add/remove port mappings" [read: S50]; `crab_nat` 0.8.1 and `natpmp` 0.5.0 |

Findings on this table:

- **Listening is passive only for announcements, which are infrequent.** A useful inventory sends one query per protocol. The query goes to a fixed group, is small, and is what an operating system's own discovery does when a person opens a printer dialog. That is a different act from addressing hosts one by one.
- **Windows and macOS each have a system service for DNS-SD.** Windows has a browse call (`DnsServiceBrowse`, asynchronous, a callback per result) [read: S17]; Apple routes all DNS-SD and Bonjour use through its own service, which is what the permission check covers [read: S1]. On Linux the daemons are Avahi or systemd-resolved [memory; read for resolved: S26]. Using the system's service avoids binding port 5353 in the product; `zeroconf` 0.18.0 wraps Bonjour or Avahi and needs those libraries installed [read: S51]. Using `mdns-sd` keeps everything in Rust and in the product's process, and needs no system library.
- **The reply is a claim.** The name, model string, and service list a device announces are text it chose. A product that stores them should store them as "claimed by the responder at this address on this network at this time", with that provenance, never as a fact.
- **A URL in an announcement must not widen reach.** An SSDP location and a WS-Discovery transport address are URLs chosen by the responder. Fetching one without checking that its host is the responder's own address, on the same link, and not a redirect to elsewhere, lets any host on the link steer the product's requests. This is the case section 502 describes, applied to the network.
- **A multicast query sent through a VPN, a guest network, or a cellular link may be silent** (part 5).

## 3. What can be asked directly, in order of how intrusive it is

Each step is something an ordinary client does. The breadth and rate of repeating it decides whether it is inventory or a scan.

| Step | What is sent | Unprivileged on | What it reveals | Notes |
| --- | --- | --- | --- | --- |
| 1. Name lookup | A DNS query to the configured resolver, or a multicast query for a `.local` name | All three | An address; the resolver's operator sees the name | On macOS, resolving a `.local` name needs the Local Network permission; resolving other names with the system resolver does not [read: S1] |
| 2. Reverse lookup | A pointer query for the address | All three | A host name, if one is published | Answered by the resolver, not by the host |
| 3. ICMP echo | One echo request to one address | Windows: the IP Helper echo functions (`IcmpSendEcho`, `IcmpSendEcho2Ex`, `Icmp6SendEcho2`) need no raw socket [read: S16, S55]. macOS: datagram ICMP sockets [read: S55 source, memory for the system]. Linux: datagram ICMP sockets, gated by a group range [read: S18, S19] | That an address answers; round-trip time | Linux: the kernel's default is "1 0", meaning nobody, not even root [read: S19]; systemd's shipped sysctl default sets "0 2147483647", with a leading `-` so a failure to set it is ignored [read: S20]. A container can add its own limits [read: S48]. Many hosts and firewalls drop echo |
| 4. TCP connect to one port | A full handshake from the ordinary socket interface | All three | Whether something listens; with a banner, more | The whole handshake completes, so the target's service sees a connection and logs it. macOS: an outgoing connection to a local address needs the Local Network permission [read: S1] |
| 5. Read a banner | Nothing further; read what the service sends first | All three | Greeting text of some services (mail, FTP, SSH, and others) [memory] | A service that speaks only after the client does gives nothing |
| 6. TLS handshake | A ClientHello (with a server name, if the caller sets one) | All three | The certificate chain: subject and alternative names, issuer, validity dates, key | A certificate read from a handshake without verification is a claim, not an identity. Parsers exist: `x509-parser` 0.18.1, and `tokio-rustls` 0.26.6 for the handshake |
| 7. HTTP request | A `GET` of `/` or of one conventional path | All three | Status, server header, a title, an interface description (part 4) | Send an honest, constant client string. A short list of named paths is a request; a list of hundreds is a scan |
| 8. Port range | Step 4 (or a half-open variant that needs raw sockets) across many ports | Connect-style only | Which ports of a host respond | See below |
| 9. Host sweep | Step 3 or 4 across an address range | Connect-style or echo only | Which addresses respond | See below |

**What a port scan is, and what tools do by default.** A scan is steps 3 to 4 repeated over a set of ports or addresses.

- **Nmap** [read: S27]: with no host-discovery option, it sends an echo request, a SYN to port 443, an ACK to port 80, and a timestamp request (IPv6 omits the timestamp); for local Ethernet targets it uses ARP or neighbour discovery instead. For unprivileged Unix users the default probes are connect attempts to ports 80 and 443. A SYN scan is the default, and falls back to a connect scan without raw-packet privilege; unprivileged users can run only connect and FTP-bounce scans. By default it scans the 1,000 most common ports per protocol, in randomised order, at a "normal" timing template (the default). It has an `--unprivileged` flag.
- **naabu** [read: S32]: the top 100 ports by default, 1,000 packets per second, a one-second timeout.
- **masscan** [read: S31]: a default of 100 packets per second; designed to reach 10 million per second with its own network stack.
- **httpx** [read: S33]: 50 threads by default.
- **ZMap** [read: S28, S29, S30]: its README says its defaults are "usually safe". Its wiki says that by default it scans as fast as the network card allows, with no congestion control, and warns that this can take down a small target network. The project's own documents disagree.

**Where the line is commonly drawn, as the published norms say it** (not an opinion of this note):

- **Permission and ownership.** The nmap manual says that although the author does not think scanning other networks is or should be illegal, administrators may complain, and "getting permission first is the best approach"; it grants permission to scan one named test host, only with nmap, a dozen scans a day at most, and not exploit or denial-of-service testing. It also says that some scripts are "intrusive and should not be run against a target network without permission" [read: S27].
- **Minimal, signalled, opt-out.** ZMap's published best practices: minimise impact by scanning no more or more often than needed, at the minimum rate; signal intent with reverse DNS, whois records, and a web page; provide an opt-out and publish the source ranges; test against your own systems first; coordinate with local IT and security; disclose results [read: S29]. Other published statements say much the same: a national cyber-security centre's principles ("minimise scanning activity", "make opt-out simple") and a scanning company's ten commandments [search: W11]. A 2025 measurement paper found that, apart from one engine, the major device-search engines offered no opt-out and most hid their identity [search: W11].
- **Authorisation before testing.** NIST's technical guide on security testing treats rules of engagement and written management approval as part of planning, and lists network discovery and port and service identification as discovery techniques [search: W14].
- **Protocol-conformant versus hostile.** The tools above separate plain connection attempts and conformant requests from scripts that exploit or crash (the nmap manual's "slight risk of crashes" paragraph; the intrusive-script warning) [read: S27].
- **What defences treat as hostile.** The sources read give no figures for intrusion-detection thresholds. [search: W14 found none.] What can be said from the tools' own documents is that breadth (many ports on one host, or one port on many hosts), speed, randomised order, and non-conformant packets are the traits the tools themselves warn about.

Taken together, the lines drawn in the published norms are: (a) your own network or permission against someone else's; (b) one named target against a range; (c) a conformant request against an exploit, fuzz, or credential try; (d) identified and rate-limited against hidden. The norms do not give a number of hosts or ports at which asking becomes scanning.

## 4. Things on a network that describe themselves well enough to become tools

| Thing | Discovery route | Description format | Does it separate reading from changing? | What an unauthenticated or unpaired client sees |
| --- | --- | --- | --- | --- |
| UPnP devices | SSDP, then the location URL [search: W2] | An XML device description listing services, each pointing to a service description of actions and state variables [memory]; control is a SOAP post [memory; `rupnp` does it: read S47] | No flag. Action names such as `Get...` and `Set...` are a convention [memory]. UPnP has a device-protection facility [search: W2 mentions device-protection headers] | The description is served to anyone who asks; control may be open or protected [memory] |
| Printers | DNS-SD service types such as `_ipp._tcp` [search: W8, from the `ippfind` manual page]; address and URI in the record | IPP over HTTP, defined by RFC 8010 and RFC 8011 (named by the `ipp` crate's README) [read: S56]; the `Get-Printer-Attributes` operation returns the printer's attributes [search: W8] | Yes, at the protocol level: the operation set separates getting attributes and jobs from creating, cancelling, or pausing [memory] | Many printers answer attribute requests without credentials [memory; the search did not confirm it] |
| Cameras | WS-Discovery probe for a video-transmitter type [search: W3, W7] | ONVIF: SOAP services with WSDL descriptions [memory] | ONVIF defines access classes, and the operations that must work with no authentication are those in a pre-authentication class; one source lists `GetSystemDateAndTime`, `GetHostname`, `GetEndpointReference`, and says these are the only ones the specification requires to work unauthenticated; another lists `GetCapabilities` as open in one server's table [search: W7, secondary] | Time, and capability data some devices allow; profiles and stream addresses need credentials in the one implementation's table [search: W7] |
| Smart-home devices | Matter: DNS-SD service `_matterc._udp` while commissionable (devices advertise about 3 to 15 minutes after power-up); operational devices use `_matter._tcp` [search: W6]. HomeKit accessories use another DNS-SD type [memory] | Matter TXT keys carry a discriminator, vendor and product, commissioning mode, and device name, per one implementation [search: W6] | Matter's data model marks attributes and commands by access; reading them needs a secure session [memory] | That the device exists, its vendor and product, and whether it is open to commissioning. Anything more needs pairing, which uses a setup code the person holds |
| SNMP agents | UDP 161 to a known address; no multicast discovery [memory] | The `system` group (description, object identifier, name, location, contact, uptime) [memory: RFC 3418]; the rest is MIB-defined | GET, GETNEXT, GETBULK read; SET writes. The community string (v1, v2c) is the credential; "public" is a convention, not a right [memory] | With the right community string, everything the agent allows. Without it, nothing |
| A router's own interface | The default gateway (part 1); UPnP IGD; HTTP on the gateway | A web page and sometimes a device description | Nothing standard | Often a login page; sometimes the description. Logging in needs a credential the person has |
| Storage and media servers | UPnP media servers over SSDP; DNS-SD types for SMB, NFS, and WebDAV file sharing [memory] | UPnP content-directory actions; SMB, NFS and WebDAV have their own protocols | UPnP: browse is read, others write. SMB and NFS: by permission, not by description | Listing exported shares without credentials is sometimes allowed; whether to try is a design question (section "What it means") |
| Well-known URL | One request to a fixed path on a known host | RFC 8615 defines the `/.well-known/` prefix [read: S38, which cites it]. For API descriptions there is **no standard path**. The OpenAPI document says an entry file "should be named" `openapi.json` or `openapi.yaml`, with no location [read: S41]; one popular Python framework serves `/openapi.json` [read: S42]; one popular Java library serves `/v3/api-docs` [read: S43]. Other conventions [memory] | OpenAPI marks no operation as safe; HTTP method semantics are all there is. Fields can be marked read-only or write-only [read: S41] | Whatever the server serves publicly |
| Existing MCP servers over HTTP | See next paragraph | A server card: identity and remote endpoints, no tools [read: S38] | The tool list, once connected, carries optional hints from the server, including a `readOnlyHint` that defaults to false [read: S40]. The server sets the hint itself | The card, if published; `tools/list` after connecting, with whatever authentication the server demands |

**MCP discovery today** [read: S38, S39, S40], and where sources disagree:

- SEP-2127, "MCP Server Cards", is marked Final in the MCP repository's file. A card describes a remote server; "cards can be hosted at any unreserved URI, with `<streamable-http-url>/server-card` reserved as the recommended location"; a domain may publish an AI Catalog at `/.well-known/ai-catalog.json` that lists cards. The detailed wire format lives in a separate extension repository.
- The draft protocol schema says the runtime `server/discover` method exists, clients may call it, and a server need not implement it. That is after a client already knows where to connect.
- Web-search summaries gave other paths (`/.well-known/mcp/server-card.json`, `/.well-known/mcp.json`, `/.well-known/mcp-server`) and a DNS text-record proposal at an `_mcp` label [search: W12]. Those come from blog posts and individual drafts. The repository file read here is the more reliable source, and the others are not adopted by it; treat them as not settled.
- No standard way for a server on a local network to announce itself by mDNS or SSDP was found.

**What this means for "becoming a tool".** What a thing says about itself can tell the product that a thing exists, what protocol it speaks, and sometimes a list of operations. It almost never says which operations only read. For each class above, deciding reading from changing is knowledge about that kind of thing, and a pack is where it lives (rule 8). An operation with no known class is a stop (rule 7).

## 5. What each operating system asks of the program and shows the person

### macOS: Local Network privacy [read: S1 unless marked]

- **Introduced** in macOS 15 (iOS 14 earlier). The person controls it in System Settings, Privacy & Security, Local Network. Three states: undetermined, allowed, denied. State is kept per user account on macOS. A device manager cannot configure it by MDM.
- **What is a local network:** an IP network on a broadcast-capable interface, "Wi-Fi and Ethernet, but not cellular or VPN"; plus all multicast addresses (224.0.0.0/4, ff00::/8) and the IPv4 broadcast address.
- **What needs the permission:** an outgoing TCP connection; sending UDP unicast, multicast, or broadcast; connecting a UDP socket; receiving UDP multicast or broadcast; resolving a `.local` name; registering, browsing, or resolving a Bonjour service. **What does not:** listening for and accepting TCP; receiving a UDP unicast; resolving other names; traffic to a local DNS server or a local proxy; browser traffic from the system web view and Safari. The check is "deep in the networking stack" and applies to all APIs, including raw BSD sockets.
- **Who is exempt:** any daemon started by `launchd`; any program running as root; and command-line tools run from Terminal or over SSH, "including any child processes they spawn". The `launchd` exemption does not cover `launchd` agents.
- **Anyone else** is blocked until the person grants it. macOS finds the **responsible code**: if an app starts a helper that makes the connection, the app is judged, named in the prompt, and recorded. For a launch agent not installed through the modern service-management API, a plist key associates the bundle. A process that exits right after a failure may never show the prompt, an Apple-acknowledged bug.
- **The first operation may fail at once:** "the system may deny the operation immediately, before the user has responded to the alert". Callers should retry or use an API that waits for connectivity.
- **No entitlement:** "The multicast entitlement isn't required on macOS." One search summary of forum posts also says it is "neither required nor supported" there and that a forum suggestion to wrap a command-line tool in an app bundle for the entitlement conflicts with the technical note [search: W1].
- **Exceptions are configurable:** two user defaults in the `com.apple.network.local-network` domain take arrays of CIDR networks, one for wired Ethernet and one for Wi-Fi interfaces. This is the only network-bound control Apple documents.
- **A search summary added** that the permission cannot be reset with the usual privacy-reset tool [search: W1]. Not verified.
- **Consequence for this product:** whether the permission applies depends on who starts the program. Started from a terminal: exempt. Started by a desktop agent application as its tool server: the agent application is the responsible code and is the one the person is asked about. A product that behaves differently by how it was started needs to say so in its output as a named condition, not fail silently.

### Windows

- **Firewall and network profiles.** Windows labels each network public, private, or domain-authenticated [read: S12]; the label is the person's or the domain's, and the documentation warns not to infer firewall state from it. A program that begins listening (a socket bound to receive) may cause the firewall to prompt the person to allow it on private or public networks; this is general Windows behaviour [memory] and does not apply to outgoing connections. The WS-Discovery interoperability note says a reply sent later than 4 seconds after the probe may be dropped by the firewall [search: W3].
- **Wi-Fi and location.** See part 1: the wireless name and access-point identifiers are behind the precise-location consent [read: S11].
- **Metered links.** The network-list API reports a connection cost with values for variable, over-data-limit, congested, roaming, and approaching the data limit [read: S14].
- **Raw sockets.** [memory] Raw sockets exist on current Windows but are restricted; the echo API above avoids them.
- **Packaged applications.** [memory] A packaged application declares network capabilities (client, private-network client and server) and a loopback exemption governs reaching its own host; not verified here.

### Linux

- **Capabilities.** `CAP_NET_RAW` is needed for raw and packet sockets [read: S23]. Datagram ICMP needs the group range above [read: S19]. The netlink manual says only a process with effective user id 0 or `CAP_NET_ADMIN` may listen to or send to a netlink multicast group [read: S21]; that text may be older than current kernels, because networking tools listen for address and link changes without privilege in practice [memory: unverified]. Reading tables by request (a dump) is not what that sentence is about.
- **Ports.** Binding below the unprivileged-port start requires root or `CAP_NET_BIND_SERVICE` [read: S19]. The multicast discovery protocols above use 5353, 1900, and 3702 as destinations; whether another program is holding the same port is a separate matter [memory].
- **No prompt.** There is no per-program network consent in the base system; a sandbox adds one (next bullet).
- **Sandboxed package formats.** [memory] Flatpak and Snap give a program network access by permission; macOS app sandboxing has client and server network entitlements; Windows packaged apps have the capabilities above. A tool server started by a sandboxed agent application inherits that application's limits. No source was read for any of these.

### What happens to each route on a VPN, a guest network, or a cellular link

| Condition | mDNS, SSDP, WS-Discovery (multicast) | Neighbour table | Directed requests (ICMP, TCP, HTTP) |
| --- | --- | --- | --- |
| VPN, full or split tunnel | Does not cross a routed tunnel in general [memory]; on macOS a VPN interface is not a local network [read: S1] | The tunnel has no link-layer neighbours | Reach whatever the tunnel routes. The address space may overlap with the local one, so the same private address can mean two places [memory] |
| Guest network, client isolation | The access point may block traffic between clients, unicast and multicast [memory]; discovery returns nothing | Empty or only the gateway | Only the gateway and beyond |
| Cellular or metered | Not a broadcast-capable network: macOS excludes it from "local" [read: S1]; the person may be paying per byte [read: S14] | None | Reach the carrier's network, often through carrier-grade NAT (part 7) |
| Public wireless in a café, hotel, or airport | Often isolated; devices there belong to strangers | Strangers' hosts | The same |

An empty result on any of these is not evidence that nothing is there. It is evidence about what the route could see. The product should report which routes it tried, which were unavailable, and why.

## 6. How a program can tell which network it is on

The aim: a permission given for one network must not follow the machine to another. No signal is both stable and unspoofable. The table says who can fake each.

| Signal | Where to read it | Stable across time? | Who can spoof it | Needs permission? |
| --- | --- | --- | --- | --- |
| Gateway's hardware address | Neighbour table plus default route (part 1) | Stable for one router; changes when the router is replaced | Anyone on the link (ARP and neighbour-discovery spoofing) [memory]; also, a router model may ship with predictable addresses [memory] | No |
| Wireless network name (SSID) | Part 1 | Stable per network, not unique: the same common name is used by many unrelated networks | Anyone can broadcast any name | Windows: yes, precise location [read: S11]. macOS: not in the page [read: S2] |
| Access-point identifier (BSSID) | Same calls on Windows [read: S11] | Changes between access points in one mesh | Anyone with a radio | Windows: yes [read: S11] |
| DHCP server address, lease domain, DNS suffix | Part 1 [read: S8] | Stable per network | A rogue DHCP server on the link | No |
| Gateway or well-known host's TLS certificate or key | A TLS handshake (part 3) | Stable until replaced | Not spoofable without the key, but a self-signed one has no authority behind it; trust on first use | No |
| Operating system's own network identity | Windows: `INetwork::GetNetworkId` returns a GUID and `GetCategory` the category; the call "returns S_OK even if the network requested has been deleted" [read: S13, S12]. The same network GUID is a member of the adapter record [read: S8]. macOS: the System Configuration service identifier per interface [memory]. Linux: NetworkManager's connection UUID, if it is running [memory] | Per machine, assigned by the system from its own signals [memory] | By whatever the system keys on | No |
| Domain-authenticated | Windows category value [read: S12] | Stable | Strongest of the above, because it is authenticated against the directory [memory] | No |

**How existing software binds trust to a network:**

- **Operating-system profiles.** Windows public/private/domain [read: S12]; macOS exceptions keyed by CIDR and interface kind [read: S1]. Both are labels that the person set once; neither is cryptographic.
- **Browsers.** Private Network Access (formerly CORS-RFC1918) required a device to answer a preflight to opt in to requests from a more-public page. It "was never able to ship" in full; Chrome kept an enterprise-policy mode and for a time a secure-contexts-only restriction. Its replacement, **Local Network Access**, gates such requests on a per-site permission the person grants, so the user, not the device, decides. It classifies each destination into three address spaces: loopback, local (private address literals, `.local` names), and public; a "local network request" is one crossing to a more private space [read: S34, S35, S36]. The two WICG repositories describe each other as the successor; one says PNA was "briefly known as" Local Network Access and the other says Local Network Access replaces PNA, so the names are confused in the sources. The Local Network Access explainer says it "has not been approved to ship in Chrome" [read: S34]. Neither binds to a particular network; both bind to a site and an address space.
- **What none of them does** is notice that the machine has moved from the person's home network to another network that uses the same address range. This product should not assume the browsers' answer applies.

## 7. Address ranges and what they mean

From the standard library's own documentation of the ranges, which cites each RFC [read: S37]:

| Range | Meaning | Standard |
| --- | --- | --- |
| 127.0.0.0/8; ::1 | Loopback: this machine | RFC 1122; RFC 4291 |
| 10.0.0.0/8; 172.16.0.0/12; 192.168.0.0/16 | Private use | RFC 1918 |
| 100.64.0.0/10 | Shared address space, used by carriers for carrier-grade NAT | RFC 6598 |
| 169.254.0.0/16; fe80::/10 | Link-local: valid on one link only; self-assigned when DHCP fails | RFC 3927; RFC 4291 |
| fc00::/7 | Unique local IPv6 | RFC 4193 (cited by the Local Network Access explainer [read: S34]) |
| 224.0.0.0/4; ff00::/8 | Multicast | RFC 5771; RFC 4291 |
| 255.255.255.255 | Limited broadcast | RFC 919 |
| 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24 | Documentation (IPv4) | RFC 5737 |
| 198.18.0.0/15 | Benchmarking | RFC 2544 and its erratum |
| 240.0.0.0/4 | Reserved | RFC 1112 |
| 2001:db8::/32 | Documentation (IPv6) | RFC 3849 [memory] |

**Why "private" does not mean "the person's own network".**

- A public wireless network at a café, hotel, airport, or conference almost always hands out private addresses. The other machines on it belong to strangers.
- A corporate network uses the same ranges; the person may be entitled to some of it and not all, and the administrator may treat any sweep as hostile.
- A cloud virtual network uses private ranges for hosts that belong to other tenants of the same account or of a peering partner.
- A carrier-grade NAT address (100.64/10) shows the machine is behind a carrier's NAT; others in that range may be other customers.
- Two networks can use the same range (a home network and a VPN that both use 192.168.1.0/24), so an address alone does not say which.
- Browsers use "private or `.local`" as the test for "local" [read: S34], which is an approximation for their purpose and no statement about who owns the network.

**IPv6: neighbour discovery against sweeping.** A /64 subnet has 2^64 addresses; a sweep cannot cover it. RFC 7707 (Informational, 2016) says scanning has been considered infeasible and argues that, with heuristics, it is possible though harder [search: W9]. What works on a link is reading the machine's neighbour cache and sending one echo request to the all-nodes multicast group, and listening for mDNS [memory]. The product should do the first and the last before anything else, and treat an IPv6 sweep as not available rather than as slow.

## 8. Rust crates

Current versions and release dates are from the registry on 2026-11-18. "Closed list" means MIT, Apache-2.0, BSD-3-Clause, Unicode-3.0, Zlib, ISC, CDLA-Permissive-2.0; a dual licence is acceptable when one side is on the list. Transitive licences and the advisory database were not checked for any crate. "Last release" is the registry's date for the newest non-yanked version, not a commit date.

### Interfaces, routes, DNS, sockets, processes

| Crate | Version | Licence | OS | Privilege | Last release | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `netdev` | 0.47.0 | MIT | Linux, macOS, Windows, Android, iOS, BSD [read: S54] | None stated | 2026-10-25 | Interfaces with type, hardware address, addresses, MTU, state, traffic counters; default interface and gateway. Linux via netlink crates; Windows via `windows-sys` IP Helper; macOS via System Configuration plist files with an API fallback. CoreWLAN link speed is an opt-in feature because it "may perform synchronous IPC". Recommended in `crab_nat`'s README for finding the gateway [read: S62] |
| `if-addrs` | 0.15.0 | MIT OR BSD-3-Clause | Unix, Windows | None | 2026-02-08 | Addresses only; used by `mdns-sd` [read: S45 manifest] |
| `network-interface` | 2.0.5 | MIT OR Apache-2.0 | Linux, FreeBSD, macOS, Windows | None | 2026-01-03 | Interfaces |
| `local-ip-address` | 0.6.13 | MIT OR Apache-2.0 | Three | None | 2026-05-19 | Local address and interface list |
| `default-net` | 0.22.0 | MIT | Three | None | 2024-02-19 | Same repository owner as `netdev`; appears to be its predecessor; not confirmed |
| `hickory-resolver` | 0.26.3 | MIT OR Apache-2.0 | Three, plus Android | None | 2026-09-10 | Stub resolver; `system-config` (default feature) reads the system's DNS servers and search domains on Unix, Windows, Apple, Android [read: S52]. Minimum Rust 1.88 |
| `listeners` | 0.6.1 | MIT | Windows, Linux, macOS, BSDs | Own processes only without privilege [read for Linux: S24; memory for the others] | 2026-08-02 | Listening ports with process id and name |
| `netstat2` | 0.11.2 | MIT OR Apache-2.0 | Three | Same | 2025-08-14 | Windows table calls, Linux socket-diagnostic netlink plus `/proc`, macOS `proc_pidfdinfo` [read: S58] |
| `sysinfo` | 0.40.4 | MIT | Three | None | 2026-11-16 | Broad; minimum Rust 1.95 |
| `rtnetlink`, `netlink-packet-route` | 0.24.0; 0.34.0 (1.0.0-rc2 on 2026-11-15) | MIT | Linux | Reads need none [memory] | 2026-11-04 | The route-netlink layer under `netdev`. Neighbour dumps are possible through it; no wrapper crate |
| `system-configuration` | 0.8.0 | MIT OR Apache-2.0 | macOS | None | 2026-08-11 | Bindings to System Configuration; the `objc2-system-configuration` (0.3.2, Zlib OR Apache-2.0 OR MIT, 2025-10-04) and `objc2-core-wlan` (0.3.2, same licence, 2025-10-04) crates are the newer bindings that `netdev` uses [read: S54] |
| `ipconfig` | 0.3.4 | MIT/Apache-2.0 | Windows | None | 2026-03-24 | Adapters and DNS |
| `windows-sys`, `windows` | 0.61.2; 0.62.2 | MIT OR Apache-2.0 | Windows | per call | 2025-10-06 | Direct calls for the IP Helper functions in part 1 |
| `if-watch` | 3.2.2 | MIT OR Apache-2.0 | Cross-platform | None | 2026-03-03 | Watches interface changes; useful to notice a network change (part 6) |
| `ll-neighbors` | 0.1.6 | Apache-2.0 | Neighbour cache lookup | None | 2021-11-17 | The only neighbour-cache crate found; last release five years ago; one address at a time, not a dump |
| `arp_fetcher` | 0.1.1 | MIT | Not stated | Not stated | 2026-07-14 | 49 downloads; not evaluated |
| `wifiscanner` | 0.5.1 | Apache-2.0 | Not stated | Not stated | 2019-10-05 | Old; lists hotspots by calling system tools |

### Sending: ICMP, DNS, TLS, certificates

| Crate | Version | Licence | OS | Privilege | Last release | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `ping-async` | 1.0.2 | MIT | Windows, macOS, Linux | None on Windows and macOS; on Linux needs the group range [read: S55] | 2026-02-03 | Windows: `IcmpSendEcho2Ex` and `Icmp6SendEcho2`; macOS and Linux: datagram ICMP sockets. Read in the published source; not run. 20,078 downloads |
| `surge-ping` | 0.9.1 | MIT | Linux datagram; "RAW elsewhere" [read: S57] | Raw sockets outside Linux, so root or an administrator there [read: S57; implication memory] | 2026-09-16 | Many targets over one socket; minimum Rust 1.85 |
| `pinger` | 2.2.0 | MIT | Three | None; runs the system `ping` program | 2026-08-31 | A process spawn per use; its README says it avoids permission trouble [read: S59] |
| `ping`, `ping-rs`, `fastping-rs`, `ping-tokio` | 0.10.0; 0.1.2; 0.2.4; 0.5.0 | MIT; MIT; MIT; **BSD-2-Clause** | Mixed | Not read | 2026-09-23; 2023-01-07; 2023-10-05; 2026-09-10 | `ping-tokio` is **not on the closed licence list** |
| `hickory-resolver` | see above | | | | | Also does reverse lookups |
| `dns-lookup` | 4.0.2 | MIT OR Apache-2.0 | Three | None | 2026-10-01 | Wraps the system `getaddrinfo` and reverse lookups |
| `x509-parser` | 0.18.1 | MIT OR Apache-2.0 | n/a | n/a | 2026-02-05 | Read a certificate's fields |
| `tokio-rustls` | 0.26.6 | MIT OR Apache-2.0 | Three | None | 2026-09-28 | Handshake; the `rustls` crate's stable version was not read, and the registry's newest non-yanked entry is a development release |
| `socket2`, `nix`, `mio` | 0.6.5; 0.31.4; 1.2.5 | MIT OR Apache-2.0; MIT; MIT | | | 2026-07-13; 2026-11-17; 2026-11-13 | Sockets and system calls |
| `pnet` | 0.35.0 | MIT OR Apache-2.0 | Three | Raw packets: elevation | 2024-05-30 | Not for an ordinary program |

### Discovery and description

| Crate | Version | Licence | OS | Privilege | Last release | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `mdns-sd` | 0.22.0 | Apache-2.0 OR MIT | macOS, Linux, Windows; v4 and v6 [read: S45] | None stated; ordinary UDP sockets | 2026-11-17 | Safe Rust, a thread plus channels, client and responder. Unicast responses (the "QU" bit) not implemented; legacy unicast replies and rate limits are. 7.96M downloads. Released the day before this note; its dependencies: `if-addrs`, `mio`, `socket2`, `socket-pktinfo`, `flume`, `fastrand` [read: S45 manifest] |
| `zeroconf` | 0.18.0 | MIT (the registry shows "non-standard" because the licence is a file; the file text is MIT [read: S51]) | Three, through the system's Bonjour or Avahi | None | 2026-04-07 | **Needs the system library**: Avahi daemon and headers on Linux, Bonjour installed on Windows [read: S51] |
| `libmdns` | 0.10.1 | MIT | | | 2025-09-06 | Responder only |
| `simple-mdns` | 0.7.0 | MIT | | | 2026-05-15 | Small implementation; not evaluated |
| `mdns` | 3.0.0 | MIT | | | 2021-01-29 | Stale |
| `ssdp-client` | 2.1.0 | MIT/Apache-2.0 | Three | None | 2025-04-25 | `search` with a target, a timeout, and a TTL; also notify and subscribe [read: S46] |
| `rupnp` | 3.0.0 | MIT/Apache-2.0 | Three | None | 2025-04-25 | Discover, read service descriptions, call actions [read: S47]. Calling an action is possible; a safe wrapper must not expose it |
| `ssdp` | 0.7.0 | MIT/Apache-2.0 | | | 2018-10-05 | Stale |
| `snmp2` | 0.5.3 | MIT OR Apache-2.0 | Three | None | 2026-11-02 | v1, v2c, v3; GET, GETNEXT, GETBULK, **SET**; says it forks an original crate "abandoned long time ago" [read: S60] |
| `async-snmp` | 0.18.1 | MIT OR Apache-2.0 | Three | None | 2026-08-19 | Pre-1.0 "its API can change"; includes SET, and an optional agent [read: S61]; minimum Rust 1.88 |
| `snmp` | 0.2.2 | MIT/Apache-2.0 | | | 2017-04-05 | Abandoned |
| `igd-next` | 0.18.0 | MIT | Three | None | 2026-10-04 | Find gateway, read external address, **add and remove mappings** [read: S50] |
| `crab_nat`, `natpmp` | 0.8.1; 0.5.0 | MIT; MIT | | | 2026-07-06; 2024-11-06 | NAT-PMP and PCP clients; `crab_nat` does not find the gateway itself [read: S62] |
| `ipp` | 8.0.0 | MIT/Apache-2.0 | Three | None | 2026-10-25 | RFC 8010 and 8011; async client with rustls by default [read: S56]. `ipp-client` 0.3.0 is from 2019 |
| `oxvif` | 0.17.0 | MIT | Three | None | 2026-09-14 | ONVIF client with WS-Discovery; its README shows credentialed calls [read: S53]; 10,302 downloads; minimum Rust 1.88 |
| `dhcproto` | 0.15.0 | MIT | n/a | n/a | 2026-05-19 | Parse and build DHCP messages |
| `rlldpctl` | 1.0.2 | not read | Linux | Reads a running `lldpd`'s socket | 2026-09-11 | Not read beyond the registry record |
| `lsnet` | 0.15.0 | **GPL-3.0-or-later** | | | 2026-11-08 | **Not on the closed licence list** |

**Gaps.** No maintained crate was found for: a cross-platform neighbour-table dump; the wireless network name; the DHCP lease; LLMNR or NetBIOS queries (parsers only); LLDP reception without privilege; a general WS-Discovery client (only camera- and printer-specific ones: `cctv-discovery` 0.3.0, `printscan-wsd` 0.1.0, `dahua-camera-discovery`, none read). Each is an adapter over the interfaces in part 1, using crates already in the workspace (`windows` for Windows; `nix` and the netlink crates for Linux; the `objc2` family for macOS).

**Crates that expose writes.** `rupnp` (actions), `snmp2` and `async-snmp` (SET), `igd-next`, `crab_nat`, `natpmp` (mappings) can each change the network's equipment. Wrapping them for discovery means exposing only the read half, and a test that proves it.

## What it means for the design

Candidate rules, each traced to a finding. None is a decision; the design is another agent's.

1. **Read the machine before sending anything.** Part 1 gives a complete local picture with no packet. A first pass that reads only local state, and reports it, is quiet and needs no network permission anywhere. (Findings: parts 1, 3.)
2. **Order the ways of looking by intrusiveness and bound each by what the person chose.** A candidate ladder: read local state; one multicast query per standard discovery protocol; directed single requests to hosts the person named or that a prior step found (name, echo, one connect, one handshake, one GET of a short named list); breadth (port ranges, sweeps) as a separate act with its own named choice and rate cap; never credential tries, exploit or fuzz traffic, or evasion. The published norms draw lines at permission, target count, request conformance, and identification, and give no numeric threshold. (Part 3.)
3. **Bind a permission to a network identity built from several signals, and treat a change as a new network.** No signal is both stable and unspoofable. A combination (gateway hardware address, DHCP server, DNS suffix, and where available the system's own network identifier and domain authentication) is harder to fake than one. Any change in the combination should drop the machine to "unknown network" and need the person's choice again. The wireless name and access-point identifier are optional inputs, never required, because Windows gates them. (Part 6.)
4. **An address range is never an entitlement.** Private ranges are used by public, corporate, cloud, and carrier networks. The entitlement should name a network identity, not a CIDR. (Parts 6, 7.)
5. **A responder's claim never widens reach.** Treat every name, model string, service list, and URL a device announces as a claim with provenance (responder address, network, time). Fetch a location URL or transport address only if its host is the responder's own address on the same link; do not follow redirects off it. This is the concern of section 502 (an input must not change where a request goes) applied to discovery. (Parts 2, 4.)
6. **Use the person's own system services where one exists.** Windows has a DNS-SD browse call; Apple's DNS-SD is the thing the permission covers; Linux has Avahi or systemd-resolved. The alternative is `mdns-sd`, in-process and with no system library. A design should weigh port conflicts and permission attribution against dependencies and portability. (Part 2.)
7. **Name the macOS permission as a stop, not a failure.** The first local-network operation may be refused before the person answers; the product cannot grant it or suppress the prompt, and what the person is asked about depends on who started the program. A named condition ("needs the operating system's Local Network permission for the program that started this one") with a retry after the person acts fits rule 7. (Part 5.)
8. **Report coverage with every result.** Which routes were tried, which were unavailable (VPN, isolated guest network, cellular, no permission), and the time. An empty result is not "nothing is there". (Part 5.)
9. **Discovery tools expose none of the writes found on the way.** UPnP actions, SNMP SET, port mappings, and a router's admin login are all writes or credentials. The built-in inspection tools stop at the description. A pack decides, per kind of thing, what is read, and an unclassified operation is a stop. (Parts 4, 8.)
10. **No credential guessing means no default community strings.** SNMP's "public" is a convention, not a right; trying it is guessing. An agent should be told a community string is needed, and the person supplies it. The same holds for the unauthenticated-only ONVIF calls: only the ones a standard says need no authentication. (Part 4.)
11. **A short, named list of paths, sent with an honest and constant client string.** No standard path exists for API descriptions; a list of conventional paths (part 4) is a few requests per host, not a scan, as long as it stays short and is data. (Parts 3, 4.)
12. **Resolve the tension with decision 0208's listener test.** A discovery client binds UDP sockets on non-loopback addresses to receive answers. It serves nobody. A rule test phrased as "no socket is bound on a non-loopback address" would forbid discovery; phrased as "no TCP listener, and no UDP socket that answers queries, on any address but loopback" it would not. `mdns-sd` can respond as well as query, so the product's use of it would need to be query-only. (Part 2, and [decision 0208](../decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md).)
13. **Where no crate exists, the adapters are small and local.** Neighbour table, wireless name, DHCP lease, and system proxy are each a call to an interface already in the workspace's reach; none justifies a new dependency. Two crates found are off the closed licence list (`ping-tokio`, `lsnet`). (Part 8.)
14. **Proxy reading is not proxy use.** Decision 0208 forbids taking a proxy silently from the environment. Reading the system proxy to show it to the person is a separate act; the design should say which. (Part 1.)

## What could not be verified

**Hosts that did not resolve or were refused from this session**, so everything that would come from them is [search] or [memory]: the RFC Editor, the IETF datatracker, `www.ietf.org`, `learn.microsoft.com`, `docs.rs`, `lib.rs`, `nmap.org`, `man7.org`, `man.archlinux.org`, `kernel.org` documentation, `wikipedia.org`, `w3.org`, the Chrome developer site, `wicg.github.io`, `upnp.org`, `onvif.org`, `pwg.org`, `openconnectivity.org`, the IANA registries, `cisa.gov`, `csrc.nist.gov`, `modelcontextprotocol.io`, GitHub repository pages and the GitHub API (403), and the RustSec archive.

**Specifically:**

- **No standard was opened.** The wire details of mDNS and DNS-SD (the exact group and port, the unicast-response flag, the source-address and TTL rules, announcement timing), WS-Discovery's schema, LLMNR, NetBIOS name service, RFC 4861, RFC 2131, RFC 6886, RFC 6887, UPnP Device Architecture and the IGD actions, RFC 3418, RFC 8010 and 8011, ONVIF's access-class table, and Matter's discovery and data model are [memory] or [search]. The group addresses and ports appear in search summaries or in the one crate source read (SSDP only).
- **Microsoft's text came from a source repository,** not the live site; a page may have been updated since. Microsoft's pages for the IP Helper functions are silent on elevation, so "no elevation" for them is [memory] and needs a run as an ordinary user on Windows.
- **Whether macOS requires the location permission for the wireless name** (CoreWLAN) is not in the page read. Whether macOS reads of other users' sockets need root is [memory].
- **The macOS behaviours above come from one Apple note** (read) and forum summaries (not opened). How the permission behaves for an MCP server started by a particular agent application, and which process counts as responsible, needs a run on macOS with that kind of launch.
- **The Linux netlink multicast-listening privilege** (manual page text, possibly dated) against what network tools do in practice: needs a run. Whether the default `ping_group_range` is "everybody" on a given distribution depends on whether systemd's shipped sysctl file is installed there. Not checked on any distribution.
- **Windows firewall prompts** (when a bound socket triggers one, and for which profile) are [memory]. The sandboxed package formats on all three systems are [memory].
- **No crate was built or run** on any system, so "works on the three systems" is what the crate's README, manifest, or source says. `ping-async` is the only one whose platform code was read. No advisory database was consulted; no crate is vouched for as sound. Transitive dependencies and their licences were not checked. Downloads and release dates are the registry's, not vouched for.
- **Activity:** repository pages (commits, stars, open issues) were refused; "last release" is the registry's date for the newest non-yanked version.
- **One search result was unusual and is not relied on:** a 2026 article summary said a newer macOS release adds a way to reset the Local Network permission, with a version number that looks wrong; the summary itself said to verify it. It is not repeated as a finding.
- **Sources that disagree:** ZMap's README against its wiki on default speed; the two WICG repositories on which name is the successor; the MCP discovery paths in search summaries against the SEP file read; the netlink manual page against practice. Each is noted where it arises.
- **The note on norms** is a sample, not a survey. The conduct statements read directly are the nmap manual's and ZMap's; the others are search summaries. No intrusion-detection vendor document was found, so no figure for what triggers an alert is given.
- **Names of real applications are avoided** per rule 9; tools and protocols are named because they are the subject.

## Sources

Tags: read = opened this session; search = search summary only.

**Apple** (read; documentation data files)
- S1 [TN3179: Understanding local network privacy](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy.md)
- S2 [CWInterface.ssid()](https://developer.apple.com/tutorials/data/documentation/corewlan/cwinterface/ssid().json)
- S3 [SCDynamicStoreCopyValue](https://developer.apple.com/tutorials/data/documentation/systemconfiguration/scdynamicstorecopyvalue(_:_:).json)
- S4 [`arp.c`, network_cmds](https://raw.githubusercontent.com/apple-oss-distributions/network_cmds/main/arp.tproj/arp.c)

**Microsoft** (read; public documentation source repositories)
- S5 [GetIpNetTable2](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/netioapi/nf-netioapi-getipnettable2.md)
- S6 [GetIpForwardTable2](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/netioapi/nf-netioapi-getipforwardtable2.md)
- S7 [GetAdaptersAddresses](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/iphlpapi/nf-iphlpapi-getadaptersaddresses.md)
- S8 [IP_ADAPTER_ADDRESSES_LH](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/iptypes/ns-iptypes-ip_adapter_addresses_lh.md)
- S9 [GetExtendedTcpTable](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/iphlpapi/nf-iphlpapi-getextendedtcptable.md)
- S10 [WlanQueryInterface](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/wlanapi/nf-wlanapi-wlanqueryinterface.md)
- S11 [Changes to API behavior for Wi-Fi access and location](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/NativeWiFi/wi-fi-access-location-changes.md)
- S12 [NLM_NETWORK_CATEGORY](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/netlistmgr/ne-netlistmgr-nlm_network_category.md)
- S13 [INetwork::GetNetworkId](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/netlistmgr/nf-netlistmgr-inetwork-getnetworkid.md)
- S14 [NLM_CONNECTION_COST](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/netlistmgr/ne-netlistmgr-nlm_connection_cost.md)
- S15 [WinHttpGetIEProxyConfigForCurrentUser](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/winhttp/nf-winhttp-winhttpgetieproxyconfigforcurrentuser.md)
- S16 [IcmpSendEcho](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/icmpapi/nf-icmpapi-icmpsendecho.md)
- S17 [DnsServiceBrowse](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/windns/nf-windns-dnsservicebrowse.md)

**Linux** (read)
- S18 [icmp(7), mirror](https://raw.githubusercontent.com/mkerrisk/man-pages/master/man7/icmp.7)
- S19 [ip-sysctl.rst](https://raw.githubusercontent.com/torvalds/linux/master/Documentation/networking/ip-sysctl.rst)
- S20 [systemd `50-default.conf`](https://raw.githubusercontent.com/systemd/systemd/main/sysctl.d/50-default.conf)
- S21 [netlink(7), mirror](https://raw.githubusercontent.com/mkerrisk/man-pages/master/man7/netlink.7)
- S22 [rtnetlink(7), mirror](https://raw.githubusercontent.com/mkerrisk/man-pages/master/man7/rtnetlink.7)
- S23 [capabilities(7), mirror](https://raw.githubusercontent.com/mkerrisk/man-pages/master/man7/capabilities.7)
- S24 [proc(5), mirror](https://raw.githubusercontent.com/mkerrisk/man-pages/master/man5/proc.5)
- S25 [proc_net_tcp.rst](https://raw.githubusercontent.com/torvalds/linux/master/Documentation/networking/proc_net_tcp.rst)
- S26 [systemd-resolved manual and `resolved.conf` manual](https://raw.githubusercontent.com/systemd/systemd/main/man/resolved.conf.xml) (also `systemd-resolved.service.xml` in the same directory)

**Scanning tools and norms** (read)
- S27 [nmap manual page source](https://raw.githubusercontent.com/nmap/nmap/master/docs/nmap.1)
- S28 [ZMap README](https://raw.githubusercontent.com/zmap/zmap/main/README.md)
- S29 [ZMap wiki, scanning best practices](https://raw.githubusercontent.com/wiki/zmap/zmap/Scanning-Best-Practices.md)
- S30 [ZMap wiki, getting started](https://raw.githubusercontent.com/wiki/zmap/zmap/Getting-Started-Guide.md)
- S31 [masscan README](https://raw.githubusercontent.com/robertdavidgraham/masscan/master/README.md)
- S32 [naabu README](https://raw.githubusercontent.com/projectdiscovery/naabu/main/README.md)
- S33 [httpx README](https://raw.githubusercontent.com/projectdiscovery/httpx/main/README.md)

**Browsers and address ranges** (read)
- S34 [Local Network Access explainer](https://raw.githubusercontent.com/WICG/local-network-access/main/explainer.md)
- S35 [Private Network Access README](https://raw.githubusercontent.com/WICG/private-network-access/main/README.md)
- S36 [Local Network Access README](https://raw.githubusercontent.com/WICG/local-network-access/main/README.md)
- S37 [Rust core library, IP address types and range documentation](https://raw.githubusercontent.com/rust-lang/rust/master/library/core/src/net/ip_addr.rs)

**MCP and API descriptions** (read)
- S38 [SEP-2127 MCP Server Cards](https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/seps/2127-mcp-server-cards.md)
- S39 [Server-card extension, discovery](https://raw.githubusercontent.com/modelcontextprotocol/experimental-ext-server-card/main/docs/discovery.md)
- S40 [MCP draft schema](https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/schema/draft/schema.ts)
- S41 [OpenAPI Specification 3.1.1](https://raw.githubusercontent.com/OAI/OpenAPI-Specification/main/versions/3.1.1.md)
- S42 [FastAPI metadata page](https://raw.githubusercontent.com/fastapi/fastapi/master/docs/en/docs/tutorial/metadata.md)
- S43 [springdoc-openapi README](https://raw.githubusercontent.com/springdoc/springdoc-openapi/main/README.md)

**Crates** (read)
- S44 crates.io record per crate, form `https://crates.io/api/v1/crates/<name>` (all crates in part 8; fetched 2026-11-18)
- S45 [`mdns-sd` README](https://raw.githubusercontent.com/keepsimple1/mdns-sd/main/README.md) and [manifest](https://raw.githubusercontent.com/keepsimple1/mdns-sd/main/Cargo.toml)
- S46 [`ssdp-client` README](https://raw.githubusercontent.com/jakobhellermann/ssdp-client/master/README.md) and [`search.rs`](https://raw.githubusercontent.com/jakobhellermann/ssdp-client/master/src/search.rs)
- S47 [`rupnp` README](https://raw.githubusercontent.com/jakobhellermann/rupnp/master/README.md)
- S48 [`surge-ping` README](https://raw.githubusercontent.com/kolapapa/surge-ping/main/README.md) (container note)
- S50 [`igd-next` README](https://raw.githubusercontent.com/dariusc93/rust-igd/master/README.md)
- S51 [`zeroconf` README](https://raw.githubusercontent.com/windy1/zeroconf-rs/master/README.md) and [licence file](https://raw.githubusercontent.com/windy1/zeroconf-rs/master/LICENSE)
- S52 [`hickory-resolver` manifest](https://raw.githubusercontent.com/hickory-dns/hickory-dns/main/crates/resolver/Cargo.toml) and [`system_conf` module](https://raw.githubusercontent.com/hickory-dns/hickory-dns/main/crates/resolver/src/system_conf/mod.rs)
- S53 [`oxvif` README](https://raw.githubusercontent.com/smiti1642/oxvif/master/README.md)
- S54 [`netdev` README](https://raw.githubusercontent.com/shellrow/netdev/main/README.md) and [manifest](https://raw.githubusercontent.com/shellrow/netdev/main/Cargo.toml)
- S55 `ping-async` 1.0.2: README and `src/platform/` read from the published crate file `https://static.crates.io/crates/ping-async/ping-async-1.0.2.crate` (unpacked, not built)
- S56 [`ipp` README](https://raw.githubusercontent.com/ancwrd1/ipp.rs/master/README.md)
- S57 [`surge-ping` README](https://raw.githubusercontent.com/kolapapa/surge-ping/main/README.md)
- S58 [`netstat2` README](https://raw.githubusercontent.com/ohadravid/netstat2-rs/master/README.md) and [`listeners` README](https://raw.githubusercontent.com/GyulyVGC/listeners/main/README.md)
- S59 [`pinger` README](https://raw.githubusercontent.com/orf/gping/master/pinger/README.md)
- S60 [`snmp2` README](https://raw.githubusercontent.com/roboplc/snmp2/main/README.md)
- S61 [`async-snmp` README](https://raw.githubusercontent.com/lukeod/async-snmp/main/README.md)
- S62 [`crab_nat` README](https://raw.githubusercontent.com/ryco117/crab_nat/main/README.md)

**Web-search summaries only** (the pages were not opened; each summary cites pages the search returned)
- W1 Apple developer forum threads on local network privacy for command-line tools and the multicast entitlement, with a 2026 article on the setting's history (`eclecticlight.co`); the macOS-reset claim there is not relied on.
- W2 SSDP search and location headers; an Irish national cyber-security centre page on internet-accessible SSDP.
- W3 WS-Discovery: Microsoft's interoperability notes on `Probe` and `ProbeMatches`.
- W4 LLMNR (RFC 4795 as Informational; port and group address) and the CISA countermeasure page.
- W5 mDNS: the RFC 6762 unicast-response rule as quoted second-hand; the multicast hardware address.
- W6 Matter commissionable and operational discovery (build-with-Matter handbook; Google's Matter primer).
- W7 ONVIF access classes, as described by firmware wiki pages and a media-server project's access table.
- W8 `ippfind` manual pages (IPP service types and TXT keys).
- W9 RFC 7707, *Network Reconnaissance in IPv6 Networks* (abstract and history).
- W10 NAT-PMP (RFC 6886) and PCP (RFC 6887): port 5351, the external-address request.
- W11 Scanner conduct: a 2025 measurement paper on device-search engines (arXiv 2412.15696), SANS ISC diaries, the scanning company's ten commandments, and the UK national cyber-security centre's principles as quoted.
- W12 MCP discovery proposals: SEP-2127 and draft IETF documents on DNS and well-known discovery.
- W13 LLDP: reserved group address and EtherType; the packet-socket manual page's capability line; a NetworkManager patch on bridge ports.
- W14 NIST SP 800-115 summaries on rules of engagement and discovery techniques.
