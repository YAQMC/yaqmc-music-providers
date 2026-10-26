@echo off
cd /d "%~dp0"
node.exe start.cjs
if errorlevel 1 pause
