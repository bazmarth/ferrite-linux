#!/bin/sh
# Installé dans la machine virtuelle par tools/scenarios/applications.txt : VLC depuis
# Debian, Brave depuis son dépôt officiel (https://brave.com/linux/).
set -eu
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y curl vlc
sudo curl -fsSLo /usr/share/keyrings/brave-browser-archive-keyring.gpg \
    https://brave-browser-apt-release.s3.brave.com/brave-browser-archive-keyring.gpg
echo "deb [signed-by=/usr/share/keyrings/brave-browser-archive-keyring.gpg] https://brave-browser-apt-release.s3.brave.com/ stable main" |
    sudo tee /etc/apt/sources.list.d/brave-browser-release.list
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y brave-browser
echo "INSTALLATION TERMINÉE"
