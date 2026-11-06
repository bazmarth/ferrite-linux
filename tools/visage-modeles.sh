#!/bin/sh
# Modèles du déverrouillage par visage (OpenCV zoo) : YuNet (détection, MIT) et SFace
# (reconnaissance, Apache 2.0). Trop lourds pour le dépôt (38 Mo) : téléchargés ici, puis
# vérifiés par leur empreinte SHA-256 avant d'être gardés.
#
#   tools/visage-modeles.sh [dossier]     par défaut : assets/visage du dépôt
# shellcheck shell=sh
set -eu

# shellcheck source=tools/lib.sh
. "$(dirname -- "$0")/lib.sh"
cd "$REPO"

dest=${1:-"$REPO/assets/visage"}
base="https://github.com/opencv/opencv_zoo/raw/main/models"
mkdir -p "$dest"

fetch() {
    path=$1
    sum=$2
    file=$dest/${path##*/}
    if [ -f "$file" ] && printf '%s  %s\n' "$sum" "$file" | sha256sum -c --status; then
        ok "${path##*/} déjà présent"
        return
    fi
    info "Téléchargement de ${path##*/}"
    curl -fsSL --max-time 300 -o "$file.partiel" "$base/$path"
    if ! printf '%s  %s\n' "$sum" "$file.partiel" | sha256sum -c --status; then
        rm -f "$file.partiel"
        die "${path##*/} : empreinte inattendue, fichier refusé"
    fi
    mv -f "$file.partiel" "$file"
    ok "${path##*/} vérifié"
}

fetch face_detection_yunet/face_detection_yunet_2023mar.onnx \
    8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4
fetch face_recognition_sface/face_recognition_sface_2021dec.onnx \
    0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79
