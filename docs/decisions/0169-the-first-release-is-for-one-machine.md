# 0169. The first release is for one machine

**Status:** Accepted. Owner decision, given on 2026-11-17.

**Date:** 2026-11-17

## Context

The roadmap runs to version 1.0 through twelve milestones. Milestone 9 is a server: many users, grants, toolsets on their own endpoints, runners on other machines, a second store. By 2026-11-17 the kernel, ten packs, the three front doors, the local engine, and the packaging existed and had been through a safety audit, and none of the server existed. The owner asked what share of the whole was done, and then set the scope of the first finished version.

## Decision

The first finished version, the one published to pip and npm, is for one machine: one person, one local engine, the packs, the tool-building flow, the approval prompt of the operating system, and the two packages.

Server mode is not in it: the second store beside SQLite for the ledger, MCP as an OAuth resource server, grants and roles for many users, toolsets on endpoints of their own, the runner link and enrollment, service accounts, and the admin console (roadmap packages 6.1 to 6.6 and 7.1 to 7.3). They follow the first release. Nothing built for them is removed, and nothing in the kernel is changed to make them harder: the scheduler's leases and the action registry's faces stay as designed.

A pack that reaches a server of its own kind is not server mode: the data pack reaching a database server, and the stream pack reaching a broker, are things one machine latches onto.

## What follows

- "Percent complete" is counted against this scope.
- The roadmap's milestone 9 moves after the first release; the documents keeper marks it so.
- Section 107's bar and the ten rules are unchanged.

## What would reopen it

The owner.
