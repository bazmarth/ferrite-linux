@echo off
rem Ferrite VM : outil graphique de test de l'ISO dans QEMU (WSL Debian).
wsl -d Debian --cd "%~dp0.." -- tools/vm-gui.sh
