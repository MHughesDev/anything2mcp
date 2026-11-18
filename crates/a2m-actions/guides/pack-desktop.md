name: pack-desktop

# The desktop pack

**Not built.** The crate has no manifest, so no mode, surface kind, or file kind is registered, and a target cannot use it yet.

The design: a desktop application is driven through the operating system's accessibility tree, by role and name, never by position on a picture. Waits end on accessibility events. Screen models, as for the web pack, hold the locators. Its sign-in is the application's own and is never copied.

Until it is built, use another route to the same system: an HTTP interface (`pack-http`), a command-line program (`pack-command`), or a file (`pack-file`). Do not draft a surface of kind `desktop`. It will not compile.
