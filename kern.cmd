@echo off
setlocal
set "DIR=%~dp0"
where python3 >nul 2>nul
if %errorlevel%==0 (
    python3 "%DIR%tools\kern_cli.py" %*
) else (
    python "%DIR%tools\kern_cli.py" %*
)
exit /b %errorlevel%
