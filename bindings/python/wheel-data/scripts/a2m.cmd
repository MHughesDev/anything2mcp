@echo off
rem The short name of the anything2mcp program on Windows. pip makes no launcher
rem for a script in a wheel's data directory, so this file is the command: it runs
rem the program installed beside it, with every argument, and returns its exit code.
if not exist "%~dp0anything2mcp.exe" (
  echo error: the `anything2mcp` program was not found beside `a2m`. 1>&2
  echo fix: reinstall it with `pip install --force-reinstall anything2mcp`. 1>&2
  exit /b 4
)
"%~dp0anything2mcp.exe" %*
exit /b %ERRORLEVEL%
