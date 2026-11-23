#!/usr/bin/env python3
"""Dessine les icônes système de Ferrite (charte « Forge »).

Une seule famille : grille de 24, trait de 1,75, bouts et angles arrondis.
Les tracés « allumés » sont en noir (#000), les parties éteintes (arcs Wi-Fi d'un
signal faible, ondes du volume…) en gris clair (#D4D4D4) : `ferrite_ui::icons::symbolic`
donne au noir la teinte voulue et estompe le gris. Aucune autre couleur.

Sorties :
- icons/SVG/symboles/NOM.svg : pictogrammes partagés (`ferrite_ui::glyphs`) ;
- icons/SVG/status-*.svg : icônes d'état (barre du haut, listes de réseaux…).

Relancer après toute retouche : python3 tools/icones.py
"""

from pathlib import Path

RACINE = Path(__file__).resolve().parent.parent / "icons" / "SVG"
ON = "#000"
DIM = "#D4D4D4"


def svg(corps: str, defs: str = "") -> str:
    # Les couleurs sont portées par le groupe et non par la balise <svg> : `symbolic`
    # ne réécrit que ce qui suit les <defs> (le masque de l'éclair garde son noir et blanc).
    tete = '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">'
    if defs:
        tete += f"<defs>{defs}</defs>"
    groupe = (
        f'<g fill="none" stroke="{ON}" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">'
    )
    return f"{tete}{groupe}{corps}</g></svg>\n"


def point(x: float, y: float, r: float = 1.35, couleur: str = ON) -> str:
    return f'<circle cx="{x}" cy="{y}" r="{r}" fill="{couleur}" stroke="none"/>'


def barre() -> str:
    """Trait oblique « éteint »."""
    return '<path d="M4 4l16 16"/>'


# ---------------------------------------------------------------- formes de base
ARCS_WIFI = [
    "M8.8 15.5a4.5 4.5 0 0 1 6.4 0",
    "M6 12.6a8.5 8.5 0 0 1 12 0",
    "M3.2 9.6a12.5 12.5 0 0 1 17.6 0",
]


def wifi(allumes: int, eteint: bool = False) -> str:
    corps = ""
    for index, arc in enumerate(ARCS_WIFI):
        couleur = ON if index < allumes and not eteint else DIM
        corps += f'<path d="{arc}" stroke="{couleur}"/>'
    corps += point(12, 19, 1.4, DIM if eteint else ON)
    return corps + (barre() if eteint else "")


BT = '<path d="M7 7.5l10 9-5 4.5V3l5 4.5-10 9"/>'
BT_CONNECTE = (
    '<path d="M8 7.5l9 8.5-5 4.5V3.5L17 8l-9 8.5"/>' + point(4.5, 12, 1.3) + point(20, 12, 1.3)
)
HP = '<path d="M3.5 9.5h3.2L11 5.5v13l-4.3-4H3.5z"/>'
ONDES = ["M14.5 9.5a3.5 3.5 0 0 1 0 5", "M17 7a7 7 0 0 1 0 10", "M19.5 4.5a10.5 10.5 0 0 1 0 15"]


def volume(ondes: int, total: int = 3) -> str:
    corps = HP
    for index in range(total):
        couleur = ON if index < ondes else DIM
        corps += f'<path d="{ONDES[index]}" stroke="{couleur}"/>'
    return corps


COUPE = HP + '<path d="M15 9.5l5 5M20 9.5l-5 5"/>'
RAYONS = [
    "M12 3.2v1.6", "M17.1 6.9l1.1-1.1", "M19.2 12h1.6", "M17.1 17.1l1.1 1.1",
    "M12 19.2v1.6", "M5.8 18.2l1.1-1.1", "M3.2 12h1.6", "M5.8 5.8l1.1 1.1",
]


def soleil(allumes: int = 8) -> str:
    corps = '<circle cx="12" cy="12" r="3.6"/>'
    for index, rayon in enumerate(RAYONS):
        corps += f'<path d="{rayon}" stroke="{ON if index < allumes else DIM}"/>'
    return corps


def batterie(pourcent: int, charge: bool) -> tuple[str, str]:
    """Corps et définitions d'une batterie remplie à `pourcent`."""
    corps = '<rect x="2.5" y="7" width="17" height="10" rx="3"/><path d="M22 10.5v3"/>'
    defs = ""
    largeur = round(12.5 * pourcent / 100, 2)
    eclair = "M12.4 8.2 8.9 12.7h2.7l-.9 3.4 3.7-4.7h-2.8z"
    if charge:
        defs = (
            '<mask id="coupe"><rect width="24" height="24" fill="#fff"/>'
            f'<path d="{eclair}" fill="#000" stroke="#000" stroke-width="3"/></mask>'
        )
    if largeur > 0:
        masque = ' mask="url(#coupe)"' if charge else ""
        corps += (
            f'<rect x="4.75" y="9.25" width="{max(largeur, 1.6)}" height="5.5" rx="1.2" '
            f'fill="{ON}" stroke="none"{masque}/>'
        )
    if charge:
        corps += f'<path d="{eclair}" fill="{ON}" stroke="none"/>'
    return corps, defs


# ---------------------------------------------------------------- pictogrammes
LUNE = '<path d="M20 14.5A8.5 8.5 0 1 1 9.5 4a6.8 6.8 0 0 0 10.5 10.5z"/>'
CLOCHE = '<path d="M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15z"/><path d="M10 21a2.2 2.2 0 0 0 4 0"/>'
CADENAS = '<rect x="5" y="10.5" width="14" height="10" rx="2.5"/><path d="M8 10.5V8a4 4 0 0 1 8 0v2.5"/>'
CLAVIER = (
    '<rect x="2.5" y="6" width="19" height="12" rx="2.5"/>'
    '<path d="M6.5 10h.01M9.5 10h.01M12.5 10h.01M15.5 10h.01M18 10h.01M6.5 14h.01M17.5 14h.01" stroke-width="2.1"/>'
    '<path d="M9.5 14h5"/>'
)
ECRAN = '<rect x="2.5" y="4" width="19" height="13" rx="2.5"/><path d="M8.5 21h7M12 17v4"/>'
GLOBE = (
    '<circle cx="12" cy="12" r="9"/><path d="M3 12h18"/>'
    '<path d="M12 3c2.6 2.6 3.8 5.6 3.8 9s-1.2 6.4-3.8 9c-2.6-2.6-3.8-5.6-3.8-9S9.4 5.6 12 3z"/>'
)
UTILISATEUR = '<circle cx="12" cy="8.5" r="3.8"/><path d="M4.8 20.5a7.2 7.2 0 0 1 14.4 0"/>'
ROUAGE = (
    '<circle cx="12" cy="12" r="8.2" stroke-width="3" stroke-dasharray="2.9 3.54" stroke-linecap="butt"/>'
    '<circle cx="12" cy="12" r="6.4"/><circle cx="12" cy="12" r="2.4"/>'
)
LOUPE = '<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>'
MICRO = '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"/>'
AVION = (
    '<path d="M12 3c.9 0 1.5.7 1.5 1.6v4.9l7 4.5v2l-7-2.2v4.4l2 1.5V21L12 20l-3.5 1v-1.3l2-1.5'
    'v-4.4L3.5 16v-2l7-4.5V4.6C10.5 3.7 11.1 3 12 3z"/>'
)
CASQUE = (
    '<path d="M4 15v-3a8 8 0 0 1 16 0v3"/>'
    '<rect x="3.5" y="14" width="4.5" height="6.5" rx="1.8"/><rect x="16" y="14" width="4.5" height="6.5" rx="1.8"/>'
)
SOURIS = '<rect x="6.5" y="3" width="11" height="18" rx="5.5"/><path d="M12 7v3"/>'
TELEPHONE = '<rect x="6.5" y="2.5" width="11" height="19" rx="2.5"/><path d="M10.5 18.5h3"/>'
CALENDRIER = '<rect x="3.5" y="5" width="17" height="15.5" rx="2.5"/><path d="M3.5 10h17M8 3v4M16 3v4"/>'
HORLOGE = '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/>'
DOCUMENT = '<path d="M14 3.5H6.5a1 1 0 0 0-1 1v15a1 1 0 0 0 1 1h11a1 1 0 0 0 1-1V8z"/><path d="M14 3.5V8h4.5M9 12.5h6M9 16h6"/>'
TELECHARGER = '<path d="M12 4v11M7.5 10.5L12 15l4.5-4.5M5 20h14"/>'
ENVOYER = '<path d="M12 15V4M7.5 8.5L12 4l4.5 4.5M5 20h14"/>'
MISE_A_JOUR = '<path d="M12 3.5v10M7.5 9L12 13.5 16.5 9"/><path d="M4 15.5v2A2.5 2.5 0 0 0 6.5 20h11a2.5 2.5 0 0 0 2.5-2.5v-2"/>'
POSITION = '<path d="M12 21s-7-6.2-7-11.5a7 7 0 0 1 14 0C19 14.8 12 21 12 21z"/><circle cx="12" cy="9.5" r="2.5"/>'
ALIMENTATION = '<path d="M12 3v8.5"/><path d="M7 6.3a7.5 7.5 0 1 0 10 0"/>'
BOUCLIER = '<path d="M12 3l7.5 3v5.5c0 4.6-3.2 8.2-7.5 9.5-4.3-1.3-7.5-4.9-7.5-9.5V6z"/><path d="M9 12l2.2 2.2L15.5 10"/>'
DISQUE = '<rect x="3" y="13" width="18" height="7" rx="2"/><path d="M5 13l2.5-7.5h9L19 13"/><path d="M16.5 16.5h.01" stroke-width="2.4"/>'
USB = '<rect x="7" y="9" width="10" height="12" rx="2"/><path d="M9 9V3.5h6V9M10.5 6h.01M13.5 6h.01"/>'
ATTENTION = '<path d="M10.3 4.2a2 2 0 0 1 3.4 0l7.4 12.8a2 2 0 0 1-1.7 3H4.6a2 2 0 0 1-1.7-3z"/><path d="M12 9.5v4"/><path d="M12 16.8h.01" stroke-width="2.4"/>'
CAMERA = '<rect x="2.5" y="7" width="13" height="10" rx="2.5"/><path d="M15.5 10.5l6-3v9l-6-3"/>'
IMPRIMANTE = '<path d="M7 9V3.5h10V9M7 17H5a2 2 0 0 1-2-2v-4a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2h-2"/><rect x="7" y="14" width="10" height="6.5" rx="1"/>'
CABLE = (
    '<rect x="8" y="3" width="8" height="6" rx="1.5"/><path d="M12 9v4M6 13h12M6 13v3.5M18 13v3.5"/>'
    '<rect x="3.5" y="16.5" width="5" height="4.5" rx="1.2"/><rect x="15.5" y="16.5" width="5" height="4.5" rx="1.2"/>'
)
PRESSE_PAPIERS = '<rect x="5.5" y="4.5" width="13" height="16.5" rx="2"/><path d="M9 4.5V3.5h6v1"/>'
OEIL = '<path d="M2.5 12C5 7.3 8.3 5 12 5s7 2.3 9.5 7c-2.5 4.7-5.8 7-9.5 7s-7-2.3-9.5-7z"/><circle cx="12" cy="12" r="3"/>'
EPINGLE = '<path d="M9 3.5h6l-1 5.5 4 4H6l4-4z"/><path d="M12 13v7.5"/>'

SYMBOLES = {
    # Connexions
    "wifi": wifi(3),
    "wifi-eteint": wifi(0, eteint=True),
    "bluetooth": BT,
    "bluetooth-connecte": BT_CONNECTE,
    "reseau": GLOBE,
    "cable": CABLE,
    "avion": AVION,
    # Son et image
    "volume": volume(3),
    "volume-moyen": volume(2),
    "haut-parleur": volume(2),
    "coupe": COUPE,
    "micro": MICRO,
    "micro-coupe": MICRO + barre(),
    "luminosite": soleil(),
    "ecran": ECRAN,
    "camera": CAMERA,
    # Modes
    "lune": LUNE,
    "nuit": '<path d="M17.5 15.5A7.5 7.5 0 1 1 8.5 6.5a6 6 0 0 0 9 9z"/><path d="M17.5 3v4M15.5 5h4M21 9.5v2M20 10.5h2"/>',
    "cloche": CLOCHE,
    "silence": '<path d="M6 16.5V11a6 6 0 0 1 9.2-5.1M18 10.5v6l1.5 2h-15"/><path d="M10 21a2.2 2.2 0 0 0 4 0"/>' + barre(),
    "economie": '<path d="M5 19.5C5 11 10 5.5 19.5 4.5 19.5 14 14 19 6 19z"/><path d="M5 19.5l7.5-7.5"/>',
    "equilibre": '<path d="M3.8 17.5a8.5 8.5 0 1 1 16.4 0"/><path d="M12 15.5V9.8"/>' + point(12, 15.8, 1.5),
    "performance": '<path d="M3.8 17.5a8.5 8.5 0 1 1 16.4 0"/><path d="M12 15.5l4.2-4.8"/>' + point(12, 15.8, 1.5),
    "clavier": CLAVIER,
    "clavier-eclaire": '<rect x="3" y="10.5" width="18" height="10" rx="2.5"/><path d="M7.5 15.5h9"/><path d="M12 3v2.5M6.5 5l1.4 1.8M17.5 5l-1.4 1.8"/>',
    "oeil": OEIL,
    "oeil-barre": OEIL + barre(),
    # Session
    "reglages": ROUAGE,
    "cadenas": CADENAS,
    "compte": UTILISATEUR,
    "comptes": '<circle cx="9" cy="9" r="3.4"/><path d="M3 20a6 6 0 0 1 12 0"/><path d="M15.5 5.8a3.2 3.2 0 0 1 0 6.2M17.5 14.5A6 6 0 0 1 21 20"/>',
    "eteindre": ALIMENTATION,
    "redemarrer": '<path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3"/><path d="M4.5 4v4.5H9"/>',
    "deconnexion": '<path d="M10 4.5H6.5a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2H10"/><path d="M15 8l4 4-4 4M19 12H9.5"/>',
    "veille": LUNE + '<path d="M14 4h4l-4 4.5h4" stroke-width="1.5"/>',
    # Appareils
    "casque": CASQUE,
    "souris": SOURIS,
    "telephone": TELEPHONE,
    "enceinte": '<rect x="6" y="3" width="12" height="18" rx="2.5"/><circle cx="12" cy="14" r="3.2"/>' + point(12, 7.3, 1.1),
    "montre": '<rect x="7" y="6.5" width="10" height="11" rx="3"/><path d="M9 6.5l.6-3.5h4.8l.6 3.5M9 17.5l.6 3.5h4.8l.6-3.5M12 10v2.2l1.4 1"/>',
    "portable": '<rect x="4.5" y="5" width="15" height="10.5" rx="1.8"/><path d="M2.5 19h19"/>',
    "imprimante": IMPRIMANTE,
    "disque": DISQUE,
    "usb": USB,
    # Réglages
    "apparence": '<circle cx="12" cy="12" r="9"/><path d="M12 3a9 9 0 0 1 0 18z" fill="#000" stroke="none"/>',
    "bureau": '<rect x="2.5" y="4" width="19" height="14" rx="2.5"/><path d="M8 15h8"/><path d="M8 21h8"/>',
    "fenetres": '<rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M3 9h18"/><path d="M15.5 6.5h.01M18 6.5h.01" stroke-width="1.8"/>',
    "energie": batterie(50, False)[0],
    "accessibilite": '<circle cx="12" cy="4.8" r="1.6"/><path d="M5 8.5l7 1.5 7-1.5M12 10v4.5M8.5 21l3.5-6.5 3.5 6.5"/>',
    "confidentialite": BOUCLIER,
    "mise-a-jour": MISE_A_JOUR,
    "a-propos": '<circle cx="12" cy="12" r="9"/><path d="M12 11v5.5"/><path d="M12 7.6h.01" stroke-width="2.4"/>',
    "applications": '<circle cx="6.5" cy="6.5" r="2.5"/><circle cx="17.5" cy="6.5" r="2.5"/><circle cx="6.5" cy="17.5" r="2.5"/><circle cx="17.5" cy="17.5" r="2.5"/>',
    "date": CALENDRIER,
    "horloge": HORLOGE,
    "position": POSITION,
    # Calendrier
    "aujourdhui": CALENDRIER + point(12, 15.2, 1.7),
    "a-venir": CALENDRIER + '<path d="M10.5 13l2.7 2.3-2.7 2.3"/>',
    "en-retard": ATTENTION,
    "taches": '<path d="M4 6.5l1.5 1.5 2.5-2.5M4 12.5l1.5 1.5 2.5-2.5M4 18.5l1.5 1.5 2.5-2.5"/><path d="M11 7h9M11 13h9M11 19h9"/>',
    # Logithèque
    "etoile": '<path d="M12 3.5l2.6 5.3 5.9.9-4.25 4.1 1 5.8L12 16.9l-5.25 2.7 1-5.8L3.5 9.7l5.9-.9z"/>',
    "installe": '<circle cx="12" cy="12" r="9"/><path d="M8 12.3l2.8 2.8L16.2 9.6"/>',
    "lecture": '<circle cx="12" cy="12" r="9"/><path d="M10 8.8v6.4l5.2-3.2z"/>',
    "pinceau": '<path d="M15 4.5l4.5 4.5L9 19.5H4.5V15z"/><path d="M12.5 7l4.5 4.5"/>',
    "code": '<path d="M8.5 8L4.5 12l4 4M15.5 8l4 4-4 4M13.5 5.5l-3 13"/>',
    "jeu": (
        '<path d="M7 8h10a4.5 4.5 0 0 1 4.4 5.5l-.8 3.6a2.3 2.3 0 0 1-3.9 1L14.5 16h-5l-2.2 2.1a2.3 2.3 0 0 1-3.9-1'
        'l-.8-3.6A4.5 4.5 0 0 1 7 8z"/><path d="M8 11v3M6.5 12.5h3"/><path d="M15.5 12h.01M17.5 13.5h.01" stroke-width="2.2"/>'
    ),
    "livre": '<path d="M12 6.5C10 5 7.5 4.5 3.5 4.5v14c4 0 6.5.5 8.5 2 2-1.5 4.5-2 8.5-2v-14c-4 0-6.5.5-8.5 2z"/><path d="M12 6.5v14"/>',
    "science": '<path d="M9.5 3.5h5M10.5 3.5v6L5 18.5a1.5 1.5 0 0 0 1.3 2.2h11.4a1.5 1.5 0 0 0 1.3-2.2L13.5 9.5v-6"/><path d="M7.5 15h9"/>',
    "grille": (
        '<rect x="4" y="4" width="6.5" height="6.5" rx="1.6"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.6"/>'
        '<rect x="4" y="13.5" width="6.5" height="6.5" rx="1.6"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.6"/>'
    ),
    "document": DOCUMENT,
    # Fichiers
    "dossier-perso": '<path d="M4 10.5L12 4l8 6.5V19a1.5 1.5 0 0 1-1.5 1.5H15v-6H9v6H5.5A1.5 1.5 0 0 1 4 19z"/>',
    "dossier": '<path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2.5h7.5A2.5 2.5 0 0 1 21 10v7.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5z"/>',
    "telechargements": TELECHARGER,
    "envoi": ENVOYER,
    "images": '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><circle cx="8.5" cy="9.5" r="1.6"/><path d="M21 15.5l-5-5-9.5 9"/>',
    "musique": '<path d="M9 18V5.5l11-2V16"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="17.5" cy="16" r="2.5"/>',
    "videos": '<rect x="3" y="5" width="18" height="14" rx="2.5"/><path d="M10 9.3v5.4l4.6-2.7z"/>',
    "corbeille": '<path d="M4 6.5h16M9 6.5V4.5h6v2M6 6.5l1 13a1.5 1.5 0 0 0 1.5 1.5h7a1.5 1.5 0 0 0 1.5-1.5l1-13"/><path d="M10 10.5v6M14 10.5v6"/>',
    "recents": HORLOGE,
    "analyse": '<circle cx="12" cy="12" r="9"/><path d="M12 3v9h9"/>',
    "modeles": '<rect x="4" y="3.5" width="16" height="17" rx="2"/><path d="M4 9h16M10 9v11.5"/>',
    "partage": '<circle cx="6.5" cy="12" r="2.5"/><circle cx="17.5" cy="6" r="2.5"/><circle cx="17.5" cy="18" r="2.5"/><path d="M8.7 10.8l6.6-3.6M8.7 13.2l6.6 3.6"/>',
    # Presse-papiers et actions
    "presse-papiers": PRESSE_PAPIERS + '<path d="M9 11h6M9 15h4"/>',
    "presse-papiers-vide": PRESSE_PAPIERS,
    "lien": '<path d="M10 14a4 4 0 0 0 5.7 0l3.3-3.3a4 4 0 0 0-5.7-5.7L12 6.3"/><path d="M14 10a4 4 0 0 0-5.7 0L5 13.3a4 4 0 0 0 5.7 5.7l1.3-1.3"/>',
    "texte": '<path d="M5 6.5h14M5 10.5h14M5 14.5h14M5 18.5h8"/>',
    "epingle": EPINGLE,
    "epingle-pleine": EPINGLE.replace('<path d="M9 3.5', '<path fill="#000" d="M9 3.5', 1),
    "fermer": '<path d="M7 7l10 10M17 7L7 17"/>',
    "retour": '<path d="M14.5 6l-6 6 6 6"/>',
    "chevron": '<path d="M9.5 6l6 6-6 6"/>',
    "valider": '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
    "rechercher": LOUPE,
    "ajouter": '<path d="M12 5v14M5 12h14"/>',
    "actualiser": '<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3"/><path d="M19.5 4v4.5H15"/>',
    "liste": '<path d="M9 6.5h11M9 12h11M9 17.5h11"/><path d="M4.5 6.5h.01M4.5 12h.01M4.5 17.5h.01" stroke-width="2.2"/>',
    "panneau": '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M15 4.5v15"/>',
    "plus-d-actions": '<path d="M6 12h.01M12 12h.01M18 12h.01" stroke-width="2.6"/>',
    # Lecture (centre de contrôle, îlot)
    "lire": '<path d="M8 5.5v13l10.5-6.5z" fill="#000"/>',
    "pause": '<rect x="6.5" y="5" width="4" height="14" rx="1.4" fill="#000" stroke="none"/><rect x="13.5" y="5" width="4" height="14" rx="1.4" fill="#000" stroke="none"/>',
    "precedent": '<path d="M18 6v12l-8.5-6z" fill="#000"/><path d="M6 6v12"/>',
    "suivant": '<path d="M6 6v12l8.5-6z" fill="#000"/><path d="M18 6v12"/>',
    "capture": '<path d="M4 8V6a2 2 0 0 1 2-2h2M16 4h2a2 2 0 0 1 2 2v2M20 16v2a2 2 0 0 1-2 2h-2M8 20H6a2 2 0 0 1-2-2v-2"/><circle cx="12" cy="12" r="3"/>',
}

# Icônes d'état : nom de fichier (sans « status- ») -> dessin.
ETATS = {
    "airplane": AVION,
    "bluetooth-on": BT,
    "bluetooth-connected": BT_CONNECTE,
    "bluetooth-off": f'<path d="M7 7.5l10 9-5 4.5V3l5 4.5-10 9" stroke="{DIM}"/>' + barre(),
    "bluetooth-searching": BT + f'<path d="M19.5 9a4.5 4.5 0 0 1 0 6" stroke="{DIM}"/>',
    "camera": CAMERA,
    "display": ECRAN,
    "download": TELECHARGER,
    "keyboard": CLAVIER,
    "location": POSITION,
    "lock": CADENAS,
    "microphone": MICRO,
    "mouse": SOURIS,
    "network": GLOBE,
    "notifications": CLOCHE,
    "power": ALIMENTATION,
    "printer": IMPRIMANTE,
    "security": BOUCLIER,
    "storage": DISQUE,
    "update": MISE_A_JOUR,
    "upload": ENVOYER,
    "usb": USB,
    "user": UTILISATEUR,
    "warning": ATTENTION,
    "wifi-off": wifi(0, eteint=True),
    "volume-0": COUPE,
}
for niveau in range(4):
    ETATS[f"wifi-{niveau}"] = wifi(niveau)
for niveau in range(1, 4):
    ETATS[f"volume-{niveau}"] = volume(niveau)
for niveau in range(6):
    ETATS[f"brightness-{niveau}"] = soleil(round(niveau * 8 / 5))
for niveau in range(6):
    corps = ""
    for index, (x, haut) in enumerate([(5, 15.5), (9.5, 12), (14, 8.5), (18.5, 5)]):
        couleur = ON if index < round(niveau * 4 / 5) else DIM
        corps += f'<path d="M{x} 19.5V{haut}" stroke="{couleur}" stroke-width="2.4"/>'
    ETATS[f"cellular-{niveau}"] = corps
ETATS["ethernet-0"] = CABLE.replace("<rect", f'<rect stroke="{DIM}"').replace("<path", f'<path stroke="{DIM}"') + barre()
for niveau in range(1, 5):
    ETATS[f"ethernet-{niveau}"] = CABLE


def main() -> None:
    dossier = RACINE / "symboles"
    dossier.mkdir(exist_ok=True)
    for nom, corps in SYMBOLES.items():
        (dossier / f"{nom}.svg").write_text(svg(corps), encoding="utf-8")
    for nom, corps in ETATS.items():
        (RACINE / f"status-{nom}.svg").write_text(svg(corps), encoding="utf-8")
    for pourcent in range(0, 101, 10):
        for charge, prefixe in ((False, "battery"), (True, "battery-charging")):
            corps, defs = batterie(pourcent, charge)
            (RACINE / f"status-{prefixe}-{pourcent:03}.svg").write_text(svg(corps, defs), encoding="utf-8")
    print(f"{len(SYMBOLES)} symboles, {len(ETATS) + 22} icônes d'état")


if __name__ == "__main__":
    main()
