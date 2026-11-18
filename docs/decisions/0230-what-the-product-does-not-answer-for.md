# 0230. What the product does not answer for

**Status:** Proposed. Owner decision: it sets what the product promises and what it leaves to others, which is the owner's to say. The owner asked on 2026-11-18 to "go through and make decisions on what this system is not responsible for"; the lines below are the lead agent's recommendations for that conversation. Nothing here is in force until the owner answers, line by line or as a whole.

**Date:** 2026-11-18

**Would change:** [section 101](../spec/1-product/101-summary.md) ("What it does not do"), [section 102](../spec/1-product/102-requirements-and-guarantees.md), [section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md) ("What stays with the developer", "What only a person does"), the README, and SECURITY.md, each by a short statement of the same lines.

## Context

The specification says what the product refuses to do (section 101) and who does what while a tool is built (section 402). It does not say, in one place, what the product does not answer for: the matters that stay with the person at the machine, the developer of a target, the agent that calls the tools, the owner of the wrapped system, or the operating system. Without that, each new reach (the desktop, scripts a developer keeps, the network of [decision 0222](0222-the-product-is-aware-of-the-network-its-machine-is-on.md)) invites the product to take on a duty it cannot keep, and a reader cannot tell a limit from a gap.

A line here is not an excuse. For each matter the table says who answers for it, and what the product still does toward it, because the product's own promises ([the ten rules](../../AGENTS.md)) stay whole: no model at run time, every write verified, secrets kept from callers, open conduct, unknown means stop.

## The lines proposed

| # | The product does not answer for | Who does | What the product still does |
| --- | --- | --- | --- |
| 1 | **Whether the person may use, inspect, or wrap a system or look at a network**: its terms, the law where they are, their employer's policy | The person | Works only through access the person already has; never gets past a protection; stops by name at a block; asks a person, never an agent, to say a system or a network is theirs to look at |
| 2 | **Whether the wrapped system is right**: its business rules, its bugs, its data | The system's owner | Proves that what it wrote is what the system then holds, and says so honestly when it cannot ("accepted, not read back") |
| 3 | **What a tool means and whether it should exist**: names, units, which record is the same record, an operation's declared effect and hazard, where a test write is allowed | The developer of the target | Lints, compiles, and gates what is declared; holds a binding to the least effect it can show; refuses what cannot be verified |
| 4 | **What the calling agent decides to do**: its plans, its prompts, its model, whether it was steered | The agent's host and the person who runs it | Assumes the agent may be steered: no input changes where a request goes; a write needs a person; an agent cannot approve, publish, or sign in |
| 5 | **Whether an agent believes what a system said.** Text from a wrapped system can carry instructions aimed at the agent | The agent's host | Marks such text as untrusted data at every door and never joins it into the product's own words |
| 6 | **Whether a person read what they approved** | The person | Proves a person was present, shows exactly what will change, and binds the approval to that change and no other |
| 7 | **The safety of the machine itself**: the operating system, an administrator, malware already running as the person, someone with the machine in their hands | The person and the operating system | Protects its own files and channel from other ordinary users and from callers; keeps secrets sealed at rest; says plainly that it does not defend against the administrator or a compromised system |
| 8 | **The security of a wrapped system or a network.** It is not a vulnerability scanner, an intrusion detector, a firewall, or a monitor | The system's and the network's owners | Reports what it meets on its way as plain findings (a link that is not encrypted, a certificate that does not verify); never goes looking for weaknesses, never tries a credential nobody stored, never exploits |
| 9 | **Keeping a tool working when the system changes** | The developer | Notices the change and stops with a named failure; never improvises around it at run time; gives the developer the evidence to redraft at build time |
| 10 | **Undoing what a system did.** Unless a restore is declared, a change is the system's to keep | The person and the system's owner | Calls such an operation `irreversible`, shows that before approval, and never sends it twice |
| 11 | **The wrapped system's availability, limits, and bills**: outages, rate limits, charges for calls | The system's owner and the person | Keeps to declared rates, counts reads, stops by name when refused, and retries nothing a declaration does not allow |
| 12 | **Being the record of the business.** The ledger records what the product did and proved. It is not a copy of the system's data, a backup, an archive, or a synchronizer | The system's owner | Keeps the least it needs to verify and reconcile, for a stated time, on the person's machine only |
| 13 | **Compliance of the person's use** with rules about personal data, finance, health, or records | The person and their organization | Keeps data on the machine, sends nothing home, redacts secrets, and gives an audit trail of its own acts that such a review can use |
| 14 | **Being the only protection where a change can hurt someone or damage equipment.** It is not a safety system | The device's maker and the person | Requires a declared hazard and range on every such point, a person's approval, and the device's own route for firmware; refuses to pass a device's protection |
| 15 | **Accounts and credentials beyond holding what a person gave**: creating accounts, choosing or rotating passwords, recovering access, answering a second factor | The person | Seals what it is given, uses it only for its owner, renews a session by the declared route, and asks the person when it cannot |
| 16 | **What other people's code does inside its own process**: a plug-in pack, a script a developer keeps, a local program a tool starts, an upstream MCP server | Whoever wrote it, and the person who switched it on | Pins it by path and digest, runs it with declared arguments only, holds it to the provider contract at the boundary, and shows a person what will run before it is published |
| 17 | **Targets other people wrote**, if [decision 0213](0213-whether-a-target-someone-else-wrote-may-be-loaded-and-found.md) is accepted: their intent and their fitness | Their author, and the person who loads them | Compiles, lints, and gates them as any target, never trusts their evidence, and makes the person publish them afresh |
| 18 | **Being an agent, or an agent framework**: planning, memory, conversation, choosing which tool to call | The agent | Serves tools to any client that speaks the protocol, and prefers none |
| 19 | **Being a service**: hosting, accounts with the project, a shared server in the first release, a catalog of integrations | Nobody; it is not offered | Runs where the person runs it |
| 20 | **Keeping itself up to date.** Nothing phones home, so nothing checks for a new version unless a person switches that on | The person | Says its version, and what a release changes, in the changelog |
| 21 | **Facts it cannot see**: the machine's clock, the network's honesty (an announcement on a local network can be forged by anyone on it), a name that resolves elsewhere tomorrow | The person and the network's owner | Treats what announces itself as a claim, never a fact; pins what it can; checks the clock in `doctor`; stops when two sources disagree |

## The choices inside these lines

Most lines only write down what the rules already imply. Six hold a real choice, and the recommendation is in the table:

1. **Line 8, a system's weaknesses.** The alternative is to stay silent about them altogether. Recommended: report what is met on the way, as a finding, and never look for more. A product that sees a password about to cross a plain link and says nothing would be less honest, and one that goes looking is a scanner.
2. **Line 9, a system that changed.** The alternative is a repair step at run time. Refused by rule 1. A build-time redraft from the stored evidence is the developer's act and is within the rules.
3. **Line 7, the administrator.** The alternative is to try to defend against the machine's administrator. Recommended: say plainly that it cannot be done, since the administrator can replace the program itself.
4. **Line 5, untrusted text.** The alternative is to filter or rewrite what a system said. Recommended: mark it and pass it whole; a filter would itself be a guess (rule 7) and would hide data a person needs.
5. **Line 12, the ledger.** The alternative is to keep full copies for convenience. Recommended: the least needed, for a stated time; a copy of the business's data is a liability the person did not ask for.
6. **Line 14, hazard.** The alternative is to refuse hazardous points altogether. Recommended: allow them under a declared hazard, a range, and a person, and say it is never the only protection.

## Consequences, if accepted

- One short statement of these lines goes into section 101 and the README, and the fuller table into section 102. SECURITY.md states lines 7 and 8 as the threat model's edge.
- A proposal that would make the product answer for one of these matters needs a record that supersedes the line.
- The network design under decision 0222 is held to lines 1, 8, and 21: looking at a network is an inventory, not an assessment, and what is found there is a claim.

## Alternatives weighed

- **No such list.** The rules imply most of it. Refused: the implication is spread over five documents, and each new reach reopens the question.
- **A disclaimer in the licence only.** The licence already disclaims warranty. It says nothing a developer can design by.
- **A longer list of everything the product is not.** Refused: a line belongs here only where someone could reasonably expect the product to answer for the matter.
