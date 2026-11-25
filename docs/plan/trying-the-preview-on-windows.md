# Trying the preview on Windows

How a person tries Anything2MCP on a Windows 11 machine before release 0.1 is published, from the Windows tester kit that CI builds, and what to write down. The same run makes three checks that only a person at the machine can make ([decision 0476](../decisions/0476-version-0-1-is-a-preview-and-the-checks-a-person-makes-move-to-before-1-0.md)): the Windows presence prompt (package 3.6), the first hour timed with a person (package 5.6), and the first use from a Windows machine (package OS5). Nothing here touches a real application: the only system it reads is the test application in the kit.

## What you need

- Windows 11 on x64, signed in at the machine itself, not over remote desktop: the presence prompt refuses a remote session by design.
- Windows Hello set up, a PIN at least (Settings, Accounts, Sign-in options). The prompt is Windows Hello's.
- A GitHub account that can open the repository's Actions page, to download the kit. No Rust, Python, or Node is needed.

## Get the kit

1. Open the repository's Actions page, choose the newest run of **CI** on `main` (or on the open pull request), and scroll to **Artifacts**.
2. Download `windows-tester-kit` and unzip it, for example to `C:\a2m-kit`. It holds `anything2mcp.exe`, `example-app.exe` (the test application, an imaginary order system), the wheel, `THIRD-PARTY-NOTICES.txt`, and this guide.
3. Windows may warn that the programs are from an unknown publisher: the kit is not signed yet. Choose to run them anyway, or unblock the zip first (its Properties, Unblock).

## Run it

Note the time when you start. Open two PowerShell windows.

In the first, start the test application and leave it running:

```powershell
C:\a2m-kit\example-app.exe      # example-app is listening on http://127.0.0.1:4010
```

In the second. The commands are written for Windows PowerShell 5.1, the one Windows 11 ships, which needs the backslashes before the quotes inside the JSON; in PowerShell 7 (`pwsh`), write the JSON without them, as `'{"order_id": "A-100"}'`.

```powershell
$env:Path = "C:\a2m-kit;$env:Path"
mkdir $HOME\a2m-try; cd $HOME\a2m-try
$env:A2M_HOME = "$HOME\a2m-try\home"
mkdir first-project; cd first-project

anything2mcp --version
anything2mcp doctor                      # what this machine lacks, if anything
anything2mcp init
anything2mcp add example --address http://127.0.0.1:4010/ --kind http
anything2mcp try example --tool example_api_orders_get --input '{\"order_id\": \"A-100\"}'
```

The try should end with `status: completed` and the order of `Example Customer One`. Then make the proven reads tools:

```powershell
anything2mcp ready example
# Run the removal that `next` names, as given, then:
anything2mcp ready example --version 1.0.0
```

The last command answers `needs_person` with a link. Open it in your browser.

## The presence prompt (package 3.6)

The page shows what will be published and which gates ran. Check each of these, and write down what happened:

1. **Cancel.** Press Carry out, and cancel the Windows Hello prompt. The page says nothing was published, and `anything2mcp ready example --version 1.0.0` still answers `needs_person`.
2. **Pass.** Press Carry out again and pass the prompt with your PIN or face. The page says it was published.
3. `anything2mcp ready example --version 1.0.0` now says the version is published, and this reads the order through the published tool:

   ```powershell
   anything2mcp call example_api_orders_get --input '{\"order_id\": \"A-100\"}'
   ```

Note the time again: from the start to this call is the first hour's measure (package 5.6, whose bar is under ten minutes).

## Connect an agent

Any MCP client that starts a program over standard input and output can use the published tool. In its configuration, with full paths:

```json
{ "mcpServers": { "anything2mcp": {
    "command": "C:\\a2m-kit\\anything2mcp.exe",
    "args": ["stdio", "--project", "C:\\Users\\YOU\\a2m-try\\first-project"],
    "env": { "A2M_HOME": "C:\\Users\\YOU\\a2m-try\\home" }
} } }
```

Ask the agent to read order A-100. It should call `example_api_orders_get` and get the same answer.

## Stop

```powershell
anything2mcp engine stop
```

Then close the test application's window with Ctrl+C. Everything the run made is under `$HOME\a2m-try`; delete that folder to remove it.

## What to write down

For each step: what you ran, what it answered (copy the text), and anything that surprised you, as plain notes. Above all:

- whether the Windows Hello prompt appeared, what it said, and whether cancelling published nothing;
- the two times, and where the time went;
- every answer that was an error, with its code;
- what `anything2mcp doctor` said.

An agent turns the notes into entries in [the copy test results](../reviews/field-tests.md) and the rows 3.6, 5.6, and OS5 of the [roadmap](roadmap.md).
