@echo off
cd /d "%~dp0"
call npm install
if errorlevel 1 exit /b 1
call npm run build
exit /b %errorlevel%
