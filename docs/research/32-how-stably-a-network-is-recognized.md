# Research: how stably a network is recognized, and how forgeable each signal is

**Status:** completed on 2026-11-23 by reading public sources. Nothing was installed, built, or run, and no packet was sent. Most of the stability claims could not be run here (this session has one Linux container and no real network), so they are claims from documentation, forum reports, and memory, tagged below. Answers research brief 36 of [11-open-research.md](11-open-research.md). Builds on note [28](28-how-a-network-can-be-seen-from-one-machine.md), part 6.

- **[read]**: the page or file was opened and read in this session.
- **[search]**: taken from a web-search summary; the page itself was not opened. Treat as a lead.
- **[memory]**: stated from knowledge, not opened. Treat as unverified.

Feeds [decision 0223](../decisions/0223-a-network-is-a-place-a-recognized-network-and-a-reach.md), [decision 0313](../decisions/0313-the-place-of-an-address-is-derived-from-its-scope-and-the-machines-subnets.md), the [network-awareness plan](../plan/network-awareness.md), and hypothesis H-44. Slice N3 is the slice that builds recognition.

## Question

Decision 0223 says a network is recognized by a fingerprint of facts the machine already holds: the default gateway's hardware address, the DHCP server identifier, the DNS search suffix, the subnet, and on wireless the SSID and BSSID. It says recognition only recalls what a person said about a network they named, and never authorizes a send. Hypothesis H-44 bets that this fingerprint is stable enough to recall a saved reach across reconnections and lease changes, and that its forgeability is acceptable.

For each signal, on Windows, macOS, and Linux: how an ordinary user reads it, how stable it is, how it can be forged, and which subset is worth keeping.

## Answer in short

- **Three signals carry a fingerprint: the gateway's hardware address, the gateway's address, and the subnet.** They can be read on all three systems with no elevation and no permission prompt (part 1), they change only when the router is replaced or reconfigured (part 2), and together they tell two networks apart far better than any one of them. None of them is unique alone: default subnets repeat across unrelated homes, and a redundancy protocol gives unrelated sites the same gateway hardware address.
- **The SSID and BSSID should not be in the fingerprint.** Windows and macOS gate them behind the person's location permission, the BSSID changes between access points of one network, and the name is not unique. They are a hint to show the person, never a key.
- **The DHCP server identifier and the DNS suffix are secondary.** They add little: the server is usually the gateway. They are not readable the same way on every system, the suffix is often empty, and a rogue DHCP server sets both. A secondary signal should be compared only when both sides have it, and a difference should stop recall.
- **The operating systems' own network identifiers are not usable as keys.** The Windows network GUID is derived from the gateway's hardware address and other facts, so it is not independent of them. The macOS service identifier names the interface's configuration, not the network. The Linux connection UUID names a saved profile, and a wired profile is reused on every wired network.
- **Forgery has two cases, and only one matters.** Someone on the person's real network gains nothing by forging: a change of the gateway's apparent address makes the network read as new, which loses a reach and grants none. Someone running a rogue network that copies a trusted network's signals can get a saved reach recalled, at low cost for a default-range home network on an open link, and at none on a password-protected wireless network they cannot join. The consequence is larger than the plan says: a recalled "may send" names addresses, and the rogue network can answer on them (part 4).
- **H-44 stays open.** The stable-subset design is well supported by documentation. Stability under real reconnections and a changed lease was not run on any system.

## 1. How each signal is read without elevation

"No elevation" means the source documents no requirement, or the claim is [memory]. A run as an ordinary user would prove it; the session ran as root.

| Signal | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Gateway hardware address | The neighbour table (`GetIpNetTable2`) joined to the default route (note 28 part 1). The system also keeps a per-network record of the gateway's hardware address and the DNS suffix in the registry under the network list; forensic parsers read `DefaultGatewayMac` and `DnsSuffix` from there [search: S9]. Reading that key as an ordinary user is [memory] | The dynamic store entry for each network service carries `ARPResolvedHardwareAddress` and `NetworkSignature = "IPv4.Router=...;IPv4.RouterHardwareAddress=..."` [read: S8]. Whether any key is hidden from an ordinary user is [memory: not] | Route-netlink `RTM_GETNEIGH`, or `/proc/net/arp` [memory], as `a2m-netstate` already reads it ([decision 0243](../decisions/0243-the-machine-reads-its-own-network-state-and-sends-nothing.md)) |
| Gateway address | Member of the adapter record, `FirstGatewayAddress`, added in Windows Vista [read: S1] | Router in the global IPv4 entry of the dynamic store [read: S8] | The default route, `RTM_GETROUTE` (decision 0243) |
| Subnet | Adapter record addresses and prefixes [read: S1] | `getifaddrs` [memory] | `getifaddrs` or the address dump (decision 0243) |
| DHCP server identifier | `Dhcpv4Server` in the adapter record; IPv4 only, and only for addresses obtained by DHCP [read: S1] | The dynamic store, or the command-line tool that prints the last DHCP packet [memory] | Not uniform. NetworkManager's DHCP configuration object exposes the options, but a report says its internal client left out `dhcp_server_identifier` [search: S14]. Other clients keep lease files [memory] |
| DNS search suffix | `DnsSuffix` in the adapter record [read: S1] | The DNS entry of the dynamic store [memory] | `/etc/resolv.conf`, or the per-link state of the resolver service (note 28 part 1) |
| SSID | `WlanQueryInterface` for the current connection returns access denied unless the person allowed precise location; a separate call, `GetConnectedSsid`, is named as the alternative, and the page does not say it needs consent [read: S2] | Needs Location Services authorization for the calling program, with no entitlement that bypasses it; a command-line program or daemon has trouble obtaining it; one report says values were empty on 15.1 even when authorized [search: S10, S11] | No gate documented; read through nl80211 or NetworkManager [memory] |
| BSSID | The same access-denied rule, and the whole Wi-Fi namespace is gated [read: S2] | The same Location Services rule as the SSID [search: S10] | As the SSID [memory] |
| System's own identifier | `NetworkGuid` in the adapter record (Windows Vista), "the GUID that is associated with the network that the interface belongs to", which may be zero if the interface provider cannot supply it [read: S1]; `INetwork::GetNetworkId` [read: S3] | The service identifier in the dynamic store key, for example `State:/Network/Service/<UUID>/IPv4` [read: S8] | NetworkManager's connection `uuid`, "a universally unique identifier for the connection" [read: S12]. The connection's `GetSettings` is not protected by polkit; its only check is the connection's `permissions` [search: S13]. Older reports of permission denied for a caller with no login session exist [search: S13] |

Three limits on all of the table:

- **A neighbour entry exists only after the machine has spoken to the gateway.** Reading the table sends nothing. If the entry is absent or has expired, the gateway's hardware address cannot be read, and the fingerprint is incomplete. The product should call that unknown and not send an ARP request itself (rule 7, decision 0243).
- **A captive-portal network can leave out the global route.** One report says the macOS global IPv4 entry is missing there [search: S8]. The fingerprint is then unavailable, which is the safe answer.
- **IPv6.** The gateway is a link-local router address learned from a router advertisement, with its hardware address in the neighbour cache [memory]. This note found no source for how often a network has IPv6 and no IPv4. The reader should fingerprint each family it finds and the match rule below should say which families must agree.

## 2. How stable each signal is

| Signal | Across a reconnection to the same network | Across a renewed lease | Across a change of access point in one network | What changes it |
| --- | --- | --- | --- | --- |
| Gateway hardware address | Same [memory] | Same [memory] | Same where one router serves the network; a mesh keeps one gateway [memory] | Replacing the router; failover between redundant routers that do not share an address |
| Gateway address | Same [memory] | Same [memory] | Same | Reconfiguring the router |
| Subnet | Same | Same: a renewal keeps the lease, and a new lease from the same server comes from the same pool [memory] | Same | Renumbering, a router reset to its factory range |
| DHCP server identifier | Same | Same | Same | Replacing the DHCP server; on managed networks, moving it between hosts |
| DNS suffix | Same | Same | Same | Server configuration; often empty on small networks [memory] |
| SSID | Same | Same | Same | Renaming; the same name is used by many unrelated networks [memory] |
| BSSID | The access point that answers may differ | n/a | Different by design: one per radio [memory] | Every roam |
| Windows network GUID | The same profile is reused; one report says a different gateway hardware address can make a new network [search: S6] | Same | Same | The system's own rules; the person deleting the profile. `GetNetworkId` succeeds "even if the network requested has been deleted" [read: S3] |

Four facts that decide the subset:

1. **Two things that look alike are not alike.**
   - Default ranges repeat. The same few address ranges are the factory setting of many home routers [memory]. A phone's tethering network is reported on one fixed `/28` with the phone as gateway at the same address every time [search: S5]. A subnet therefore says what kind of network it is, not which one.
   - A redundancy protocol gives the gateway a virtual hardware address that is fixed by the protocol and the group number, `00-00-5E-00-01-XX`, so unrelated sites share it when they use the same group number [search: S7]. On such a network the gateway's hardware address is stable across a failover and not unique across sites.
   - The gateway's hardware address alone is therefore not unique either. The three together, with the address of the gateway, are far more selective, but still not unique in the cases above.
2. **The Windows identity is built from the same facts.** Reports say the system uses the gateway's hardware address, and for a domain, the DNS suffix, to tell networks apart [search: S6]. Microsoft's own training page says the service collects "IP addresses and MAC address data from important network components, like routers and gateways" [search: S6]. A forensic test on one older version found a second network recorded with the first network's gateway hardware address [search: S9]. So the GUID adds convenience and a person-visible label, and no independent evidence.
3. **The macOS identifier names the wrong thing.** A service identifier names the interface's configuration. The same Wi-Fi service is in force on every Wi-Fi network the Mac joins. The signature string in the same entry, `IPv4.Router=...;IPv4.RouterHardwareAddress=...`, is what carries the network [read: S8], and it is the same two facts as above. Apple's engineer, in the thread read, says to treat a change notice as a trigger to re-read the store and not to parse the payload [read: S8]. The source of the signature code could not be reached.
4. **The Linux identifier names a saved profile.** A profile is something the person or the system created. Where the system made one automatically for a wired interface, it is reused on every wired network [memory: this was not checked against the source and should be]. NetworkManager may also be absent, as on servers and minimal installs [memory].

Address privacy does not break any of this. A device's randomized Wi-Fi hardware address changes this machine's own address, not the gateway's, and Apple's pages say the address is fixed per network by default, rotating only when the person chooses [search: S15]. It does affect a design that would fingerprint with this machine's own hardware address. The product should not.

## 3. How each signal is forged

Two attackers are different and the plan should name them.

- **Attacker A is on the same network the machine is on.** The goal can only be to make the machine see a different fingerprint than it should, since it is already on this network.
- **Attacker B runs a different network and wants it taken for one the person trusts.** B must get the machine onto B's link first, by an access point with the same name, a cable, or a tethered device, and then copy the signals.

| Signal | How a forger sets it | Cost | Note |
| --- | --- | --- | --- |
| Gateway hardware address | Answer the neighbour query with the chosen address; the address resolution protocol has no authentication and a host accepts an unsolicited reply and overwrites its cache [search: S16]. Or, for B, give B's own router the trusted router's hardware address | Low on a link without switch protection. Dynamic ARP inspection at the switch checks replies against DHCP snooping bindings, and trunk ports are often trusted by default [search: S16]. Home and café networks do not have it [memory] | For B: a router's LAN hardware address is often near its radio's BSSID, which every receiver in range sees [memory: unverified]. So the value may be guessed |
| Gateway address, subnet | Hand out the chosen range and router with DHCP, or configure a static link | Trivial for B | Default ranges are public knowledge |
| DHCP server identifier | A rogue server that answers first or on a link without protection. RFC 3118 authentication exists, needs a shared secret, and is not widely used [search: S17]. Switch-side snooping is the usual control [search: S17] | Low for B | Visible to anyone who has been on the network [memory] |
| DNS suffix | Set by the DHCP server's option 15 or 119. RFC 3397 itself warns that a rogue server can redirect traffic through the search list and calls it a more fruitful attack than a false DNS server [search: S18] | Low for B | Same observation as above |
| SSID | Broadcast any name | Trivial | |
| BSSID | Set the radio's address | Trivial for any radio | Detection of a lookalike access point is possible only for software watching the air [search: S19] |
| Windows network GUID, macOS service identifier, Linux connection UUID | None needed: the system assigns them from its own signals, or the person created them | As forging the signals they derive from; the Linux UUID is not an input at all | Not independent evidence |
| Router advertisement (IPv6 gateway) | Any node on the link can send one without proof of authority; the control is a filter at the switch [search: S20] | Low on a link without a filter | RFC 7113 describes evasions of the filter [search: S20] |

Results by case:

- **A against its own network.** A can ARP-spoof the gateway's hardware address so the machine believes the router has A's address. The fingerprint then differs from the saved one, so recognition fails and the reach is not recalled. The result is the safe one: a lost reach, a re-ask. This is a way to cause that, not a way to gain from it. (Inference; not tested.)
- **B imitating a protected wireless network.** B cannot complete the link-layer handshake without the network's password, so the machine does not join. Recognition never runs. Link-layer protection guards the gate; recognition sits behind it.
- **B imitating an open network, or a wired one B can plug into.** Nothing at the link layer proves which network it is. For a default-range home network, B needs only the range, a guessed or copied router hardware address, and the name. This succeeds at low cost. Recognition then recalls the saved reach.

## 4. What the recalled reach lets B do

Decision 0223 says a forged fingerprint "can only impersonate a network the person already trusted to a given degree". That is true, and it understates the effect. A reach holds two sets of places: places the engine may look at, and places it may send to. A saved "may send" names addresses on the network. Once B has the machine on B's link, B can answer at any of those addresses. A request the person approved for a device at one address then goes to B's host. Rung 0 reading is not affected, since it reads only local state.

So the bet in H-44 holds in the narrow form: recognition raises no rung and sends nothing by itself. A recalled send can still reach the wrong host. This follows from decision 0223's own design and is not a flaw in the fingerprint.

The size of the risk depends on what is sent. A read-only request to an address wrongly answered is low. A write, or a request carrying a credential, to a host that is not the device the person meant, is not. The defence belongs on the host and not on the fingerprint: a TLS certificate or key pinned when the person named the reach, or a device identifier. Note 28 already marks TLS pinning as the one signal a forger cannot copy without the key [read: note 28 part 6]. Whether the first reach should bind a send to a host identity was not researched here beyond this point.

## 5. Which subset to keep

- **Core, always read, compared for equality:** the gateway's hardware address, the gateway's address, and the subnet with its prefix, for each address family present. All three can be read on all three systems without elevation or a prompt.
- **Secondary, recorded and compared only when both sides have a value:** the DNS suffix, and the DHCP server identifier where it can be read. A secondary that differs on both sides means "not the same network". A secondary missing on one side is not a difference, because Linux cannot always read it and many networks have no suffix.
- **Hints, shown to the person and never compared:** the SSID, and the BSSID where the system gives them with consent. They help the person recognize a network in the naming dialogue. They are not read at all while the permission is missing, as decision 0243 already does.
- **Not used as keys:** the Windows network GUID, the macOS service identifier, the Linux connection UUID. The first may be shown as a label.
- **Not used at all:** this machine's own hardware address.

Misfires and what each costs:

| Case | What happens | Cost |
| --- | --- | --- |
| Router replaced, or the same router reset | Fingerprint changes; recognized as new | One re-ask. The safe direction |
| The same network seen through a different gateway hardware address (a redundant pair without a shared address, or a router that answers differently on its wired and wireless sides) | Treated as new | One re-ask each. Whether such routers exist was not researched [memory only] |
| Two unrelated networks with the same default range and the same virtual gateway hardware address | Treated as one | A reach is recalled on the wrong network. This is the false match. A secondary difference (the DNS suffix) lowers it only on networks that have one |
| Gateway hardware address unreadable (no neighbour entry yet) | Fingerprint incomplete: unknown | The person is shown an unnamed network. Rung 0 only |

The false match is the risk to measure. A false split costs a prompt. The false match restores a reach on a network the person never named, and the test for it needs two networks with the same range and a shared virtual gateway hardware address.

## 6. A test that can run

H-44 was set as "two-networks stand with one machine moving between them". Reading note 28 and the sources above, this can be built for Linux only in CI:

- Two network namespaces joined by a virtual link, each with a router namespace that holds a chosen gateway hardware address, subnet, DHCP-style configuration, and DNS suffix. The reader of decision 0243 already runs in a namespace.
- Cases: same router across a reconnect (interface down and up); a renewed lease with the pool address changed inside the subnet; the gateway's hardware address changed; two networks with one range and one virtual hardware address; a secondary signal absent on one side; the neighbour entry absent.
- A forgery run that copies each signal and confirms nothing is raised and nothing is sent. It would show the match, not the consequence described in part 4.

This cannot test Windows or macOS readers, wireless gating, or real DHCP servers. Those wait for the real-systems session (roadmap 9.5).

## What this changes in the design

Proposals. The lead applies them.

1. **[Plan network-awareness.md, the "Network" entry and its fingerprint list; decision 0223, the second bullet of the Decision.]** Replace "a fingerprint of facts the machine already knows (gateway hardware address, DHCP server id, DNS suffix, subnet, SSID/BSSID)" with: "a fingerprint of the default gateway's hardware address, the default gateway's address, and the subnet with its prefix, for each address family the interface has; the DNS suffix and the DHCP server identifier are recorded as secondary signals and compared only when both sides have a value; the SSID and BSSID are shown to the person as hints and are never part of the fingerprint". Reason: parts 1, 2, 5.
2. **[Plan network-awareness.md, "Recognizing a network".]** Add: "A match needs every core signal equal. A secondary signal that has a value on both sides and differs makes the network not recognized. A core signal that cannot be read makes the network unknown. The system's own network identifiers are not signals."
3. **[Plan network-awareness.md, "Spoofability, stated plainly"; decision 0223.]** Replace "a forged fingerprint can only impersonate a network the person already trusted to a given degree" with: "a forged fingerprint can have a saved reach recalled on a network the person never named. Recognition does not send and does not raise a rung. A recalled place to send to can be answered by a host on the forging network." Add that password-protected wireless and a person-chosen wired port stop a copy from joining at all.
4. **[Decision 0225 and the plan's reach record.]** Proposed, for a later slice: a saved "may send" names a place and, where the thing can show one, a host identity (a pinned certificate or a device identifier) read when the person named it, and a send to a recalled place whose identity does not match stops with a named failure. Not designed here.
5. **[Plan network-awareness.md, the reach record.]** Proposed: store which signals were read and equal at naming time, and show "recognized by gateway and subnet" or "recognized by gateway, subnet, and DNS suffix" to the person with the reach, so a recall made on fewer signals is visible.
6. **[Hypothesis H-44.]** Add the false match case (same range, same virtual gateway hardware address) and the unreadable-neighbour case to the test, and name the Linux namespace stand as the first run, with Windows and macOS left to the real-systems session.
7. **[Section 218, rung 0 text.]** Add: "Reading the gateway's hardware address reads the system's neighbour table and sends nothing. If the system holds no entry, the fingerprint is incomplete, and the product does not ask the network for one."
8. **[Decision 0313, "What this does not do" or the technical reference's entry for `NetworkPlace`.]** Add: "`local_network` says an address is inside a subnet of an interface that is up. It does not say which network. Two networks that share a range give the same place."

## What could not be verified

- **Nothing was run.** No signal was read as an ordinary user on any system, and no reconnection, renewed lease, or roam was observed. All stability statements in part 2 come from documentation, forum reports, or memory. The session ran as root with no real network.
- **Hosts that did not load:** `learn.microsoft.com` pages on network location awareness, `opensource.apple.com` (so the signature code was not read), `networkmanager.dev` and `developer.gnome.org` (so NetworkManager's property and permission texts came from search summaries and one source file), `rfc-editor.org` and `datatracker.ietf.org` (so the RFC statements on DHCP renewal, the server identifier, router advertisements, and virtual hardware addresses are from search summaries or memory), the Plaso source page, and the forum pages the search results named. If these hosts are allowed in the session's network settings, the part-1 claims can be checked in one pass.
- **How Windows decides two connections are one network.** What is in the sources is community tests and a training page. Microsoft's own description was not read. Whether the registry values named in part 1 are readable by an ordinary user, and what the `FirstNetwork` value holds, were not confirmed.
- **Whether macOS read of `NetworkSignature` needs any permission,** and whether the key is stable across macOS versions: one forum dump only.
- **Wired profile reuse on Linux,** DHCP lease file locations and their readability, whether `dhcp_server_identifier` is exposed by the current NetworkManager's default client, and `GetSettings` as an ordinary user on a current version: [memory] or old reports.
- **Whether a router's LAN hardware address is derivable from its BSSID** and whether a router answers with different hardware addresses on its wired and wireless sides: [memory], not checked.
- **Which address ranges are factory defaults, how common a shared virtual gateway hardware address is, and how many networks have no DNS suffix:** [memory]. No count was found.
- **Whether the Wi-Fi security type of the current link can be read without the location permission,** which would let the product tell a protected link from an open one. Not looked up.
- **IPv6-only networks:** not researched past router advertisement forgery.

## Sources

Read directly (2026-11-23):
- S1 [IP_ADAPTER_ADDRESSES_LH](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/iptypes/ns-iptypes-ip_adapter_addresses_lh.md): `NetworkGuid`, `Dhcpv4Server`, `DnsSuffix`, `FirstGatewayAddress`.
- S2 [Changes to API behavior for Wi-Fi access and location](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/NativeWiFi/wi-fi-access-location-changes.md)
- S3 [INetwork::GetNetworkId](https://raw.githubusercontent.com/MicrosoftDocs/sdk-api/docs/sdk-api-src/content/netlistmgr/nf-netlistmgr-inetwork-getnetworkid.md)
- S4 [CNCopyCurrentNetworkInfo](https://developer.apple.com/documentation/systemconfiguration/cncopycurrentnetworkinfo.md): location authorization is a requirement; the page lists iOS and Mac Catalyst and shows no macOS availability.
- S8 [How to detect IPv4 address change on macOS using SystemConfiguration framework](https://developer.apple.com/forums/thread/113446): a dump showing `NetworkSignature`, `ARPResolvedHardwareAddress`, `Router`, and the global entry.
- S12 [NetworkManager `nm-setting-connection.c`](https://raw.githubusercontent.com/NetworkManager/NetworkManager/main/src/libnm-core-impl/nm-setting-connection.c): the first 100,000 characters of 138,108 were read.
- Note 28's sources S1 to S54, as cited there, for the read paths in part 1: [28](28-how-a-network-can-be-seen-from-one-machine.md).

Known only from search results (pages not opened):
- S5 [Personal Hotspot subnet reports](https://discussions.apple.com/thread/255675443) and the other forum threads in that search.
- S6 [Windows network location awareness: how a network is identified](https://learn.microsoft.com/en-us/training/modules/explore-microsoft-defender/5-examine-network-location-profiles), [a pfSense thread](https://forum.netgate.com/post/846416), and the [Windows 10 network-switching thread](https://techcommunity.microsoft.com/t5/windows-10/mechanics-of-how-windows-10-networking-determines-when-to-switch/m-p/3585088).
- S7 [VRRP, RFC 5798](https://www.rfc-editor.org/rfc/rfc5798) (obsoleted by [RFC 9568](https://datatracker.ietf.org/doc/rfc9568/)), and [an Extreme Networks glossary entry](https://documentation.extremenetworks.com/exos_commands_22.2/_Common/Glossary/g_virtual_router_mac.shtml).
- S9 [Plaso's network-list plugin](https://plaso.readthedocs.io/en/latest/_modules/plaso/parsers/winreg_plugins/networks.html) and [a forensic forum discussion of network connection analysis](https://www.forensicfocus.com/forums/general/windows-network-connection-analysis/paged/2/).
- S10 [Apple Developer Forums: SSID missing since macOS 14.4](https://developer.apple.com/forums/thread/756344), [BSSID on macOS 14.6.1](https://developer.apple.com/forums/thread/774397).
- S11 [CoreWLAN scanning does not expose SSIDs on 15.1](https://developer.apple.com/forums/thread/769950).
- S13 [NetworkManager merge request on `GetAppliedConnection`](https://gitlab.freedesktop.org/NetworkManager/NetworkManager/-/merge_requests/634), [the NetworkManager mailing-list thread of 2014](https://mail.gnome.org/archives/networkmanager-list/2014-August/msg00064.html), and [the connection interface documentation](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.Settings.Connection.html).
- S14 [NetworkManager issue 426, incomplete DHCP options](https://gitlab.freedesktop.org/NetworkManager/NetworkManager/-/issues/426), [DHCP4Config](https://networkmanager.dev/docs/api/1.46/gdbus-org.freedesktop.NetworkManager.DHCP4Config.html).
- S15 [Apple platform security, Wi-Fi privacy](https://support.apple.com/guide/security/secb9cb3140c), [Apple, private Wi-Fi addresses](https://support.apple.com/en-al/102509).
- S16 [Kaspersky on ARP spoofing](https://encyclopedia.kaspersky.com/glossary/arp-poisoning-arp-spoofing/) and [SecureW2 on ARP spoofing](https://securew2.com/blog/arp-spoofing): the claims about RFC 826 and dynamic ARP inspection are from these secondary pages.
- S17 [RFC 3118, Authentication for DHCP messages](https://www.rfc-editor.org/rfc/rfc3118) and [a course paper on DHCP](https://www.eecs.yorku.ca/course_archive/2019-20/F/4482/Team7_DHCP.pdf).
- S18 [RFC 3397, DHCP domain search option](https://www.rfc-editor.org/rfc/rfc3397.html).
- S19 [MITRE ATT&CK T1557.004, evil twin](https://attack.mitre.org/techniques/T1557/004/).
- S20 [RFC 6105, RA-Guard](https://www.rfc-editor.org/rfc/rfc6105) and [RFC 7113](https://datatracker.ietf.org/doc/html/rfc7113).
