@echo off
rem poecraft local web app: builds (first time) and starts the server, then opens the browser.
cd /d "%~dp0"
if not exist target\release\poecraft-server.exe cargo build --release -p poecraft-server
start "" http://127.0.0.1:8790
target\release\poecraft-server.exe
