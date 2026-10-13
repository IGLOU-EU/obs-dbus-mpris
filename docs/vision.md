# Vision

## En une phrase

`obs-dbus-mpris` ajoute à OBS Studio sous Linux un type de source, « Lecture en cours (MPRIS) », qui affiche ce que joue un lecteur multimédia en le lisant via MPRIS sur le bus de session D-Bus.

## Problème

Afficher le morceau en cours dans une scène OBS sous Linux passe aujourd'hui par des plugins généralistes, Tuna par exemple, qui se configurent dans une fenêtre globale du menu Outils. On veut l'inverse : un plugin qui n'ajoute qu'une source, et dont tout le réglage se fait dans les propriétés de cette source.

## Pour qui

Une personne qui diffuse ou enregistre avec OBS sous Linux (Flatpak ou natif) et qui écoute sa musique avec un lecteur compatible MPRIS : mpv, un navigateur (Chrome, Firefox), Strawberry, Elisa.

## Ce que le plugin apporte

- **Une seule chose ajoutée à OBS** : un type de source. Aucune entrée dans Outils, aucun dock, aucun réglage global, aucun fichier écrit sur disque, aucun accès réseau sauf option explicite.
- **Tout se règle par source**, dans ses propriétés : lecteur suivi, éléments affichés, couleurs, tailles.
- **Une source autonome** : miniature, textes et barre de progression sont dessinés par la source elle-même, sans assembler de sources texte et image à la main.
- **Plusieurs sources possibles**, chacune avec son lecteur et son style.

## Périmètre

- Linux uniquement, puisque MPRIS repose sur D-Bus.
- Lecture seule des interfaces `org.mpris.MediaPlayer2` et `org.mpris.MediaPlayer2.Player`.
- OBS Studio 32 et suivants.
- Deux builds : un `.so` natif et une extension Flatpak de `com.obsproject.Studio`.
- Distribution par releases sur une forge Git, choisie plus tard (archive du `.so` et bundle `.flatpak`).
- Validation en Flatpak. Le build natif est produit, sa validation dans un OBS natif est reportée.

## Non-objectifs

- Piloter le lecteur depuis OBS (lecture, pause, piste suivante).
- Windows et macOS.
- Toute source de données autre que MPRIS pour savoir ce qui joue (API Spotify, Last.fm, lecture de pages web). Seule exception, en option : chercher en ligne une pochette que le bac à sable Flatpak empêche de lire (B-18).
- Écrire des fichiers texte ou image destinés à d'autres sources.
- Paroles, visualiseur audio.
- Toute interface hors des propriétés de la source.
- Publication sur Flathub ou AUR (pas exclue pour plus tard, mais hors du but actuel).

## Contraintes de conception

- **Data oriented, single point of truth** : chaque donnée a un seul propriétaire, tout ce qui se dérive est dérivé au lieu d'être stocké. Détail dans [architecture.md](architecture.md).
- **TDD** : la logique (sélection du lecteur, temps, décodage, mise en page) est écrite test d'abord, hors d'OBS.
- **KISS** : chaque ajout répond à un besoin constaté. Ce qui relève du « au cas où » attend dans la liste « plus tard » de la [feuille de route](feuille-de-route.md).
- **Licence GPL-2.0-or-later**, comme OBS, dont le plugin lie la bibliothèque `libobs`.

## Noms

| Élément | Valeur proposée |
|---------|-----------------|
| Plugin et fichier `.so` | `obs-dbus-mpris` |
| Identifiant interne de la source | `mpris_now_playing` |
| Nom affiché (fr-FR) | Lecture en cours (MPRIS) |
| Nom affiché (en-US) | Now Playing (MPRIS) |
| Extension Flatpak | `com.obsproject.Studio.Plugin.DbusMpris` |
