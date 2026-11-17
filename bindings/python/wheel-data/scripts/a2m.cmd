@echo off
rem The short name of the anything2mcp program on Windows. pip makes no launcher
rem for a script in a wheel's data directory, so this file is the command: it runs
rem the program installed beside it, with every argument, and returns its exit code.
rem The folder is this file's own, never the working folder: cmd can give %~dp0 as
rem the working folder when the file is named in quotes and found on the search
rem path, so the folder must hold this file before its program is run.
if not exist "%~dp0a2m.cmd" (
  echo error: the folder of `a2m` could not be told apart from the working folder. 1>&2
  echo fix: run `a2m` without quotes, or run `anything2mcp`. 1>&2
  exit /b 4
)
if not exist "%~dp0anything2mcp.exe" (
  echo error: the `anything2mcp` program was not found beside `a2m`. 1>&2
  echo fix: reinstall it with `pip install --force-reinstall anything2mcp`. 1>&2
  exit /b 4
)
"%~dp0anything2mcp.exe" %*
exit /b %ERRORLEVEL%
