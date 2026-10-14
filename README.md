<p align="center"><img src="assets/logo/logo.svg" alt="" width="128"></p>

# obs-dbus-mpris

*This README is also available in [English](README.en.md).*

Un plugin pour OBS Studio sous Linux qui ajoute une seule chose : une source « Lecture en cours (MPRIS) », qui affiche ce que joue votre lecteur multimédia. Pochette, titre, artiste, album et progression sont lus sur le bus de session D-Bus, par l'interface MPRIS que proposent mpv, VLC, les navigateurs, Strawberry, Elisa et bien d'autres.

## Ce que fait le plugin

- **Une source, rien d'autre.** Pas d'entrée dans le menu Outils, pas de dock, pas de réglage global, aucun fichier écrit sur disque. Tout se règle dans les propriétés de la source, et plusieurs sources peuvent suivre chacune leur lecteur, avec leur style.
- **Une source complète.** Miniature, titre, sous-titre « Album - Artiste », barre de progression, position et durée (ou temps restant) sont dessinés par la source elle-même. Chaque élément s'affiche ou se masque.
- **Un rendu soigné.** Miniature à gauche ou à droite, ronde ou carrée, coins arrondis, quatre couleurs avec transparence, trois tailles de police. Les textes trop longs défilent, et les caractères japonais, chinois ou coréens s'affichent dès qu'une police les contient.
- **Une sélection du lecteur pensée pour le direct.** Au démarrage, la source retrouve votre lecteur habituel, puis ne change plus d'elle-même (voir plus bas).
- **Une pochette en ligne, seulement si vous le demandez**, pour les lecteurs dont la pochette est illisible depuis OBS Flatpak.

## Installer

Il faut OBS Studio 32 ou plus récent, sur Linux x86_64. Les fichiers à télécharger sont attachés à chaque [release](https://gitlab.com/iglou.eu/obs-dbus-mpris/-/releases).

### Avec OBS en Flatpak

Téléchargez `obs-dbus-mpris-<version>-x86_64.flatpak`, puis :

```sh
flatpak install --user obs-dbus-mpris-<version>-x86_64.flatpak
flatpak override --user --talk-name='org.mpris.MediaPlayer2.*' com.obsproject.Studio
```

La seconde commande autorise OBS à parler aux lecteurs sur le bus de session. Une extension Flatpak ne peut pas accorder cette permission elle-même, et OBS ne la demande pas par défaut. Redémarrez ensuite OBS.

Pour mettre à jour, installez le nouveau bundle avec `flatpak install --user --reinstall`. Pour désinstaller : `flatpak uninstall --user com.obsproject.Studio.Plugin.DbusMpris`.

### Avec OBS installé nativement

Téléchargez `obs-dbus-mpris-<version>-x86_64-linux.tar.gz`, puis :

```sh
mkdir -p ~/.config/obs-studio/plugins
tar -xzf obs-dbus-mpris-<version>-x86_64-linux.tar.gz -C ~/.config/obs-studio/plugins/
```

Il faut une glibc 2.36 ou plus récente (Debian 12, Ubuntu 24.04 et suivants). Pour désinstaller : `rm -r ~/.config/obs-studio/plugins/obs-dbus-mpris`. Le plugin est développé et validé avec OBS Flatpak, cette version native n'a pas encore été essayée dans un OBS natif.

### Polices

Pour les titres en japonais, chinois ou coréen, installez une police CJK sur votre système : `noto-fonts-cjk` (Arch), `fonts-noto-cjk` (Debian, Ubuntu), `google-noto-sans-cjk-fonts` (Fedora). OBS Flatpak utilise les polices de l'hôte. Une police emoji (`noto-fonts-emoji`, `fonts-noto-color-emoji`) donne l'image par défaut 🎼 quand une pochette manque, sinon l'emplacement reste uni.

## Utiliser

Ajoutez une source « Lecture en cours (MPRIS) » à une scène. Ses propriétés sont rangées en cinq groupes :

- **Lecteur** : le lecteur suivi, une entrée par instance en cours, libellée par ce qu'elle joue, et la sélection automatique.
- **Éléments affichés** : chaque élément à afficher ou non, le temps restant à la place de la durée, et la pochette en ligne.
- **Disposition** : largeur totale (la hauteur suit les polices), miniature à droite, miniature ronde, écart entre la miniature et le panneau, rayon des coins, épaisseur de la barre.
- **Texte** : les trois tailles de police, la vitesse et la pause du défilement (0 le désactive, les textes trop longs se terminent alors par « … »), et le message affiché quand rien ne joue.
- **Couleurs** : fond, texte, remplissage et piste de la barre, toutes avec transparence.

### Choix du lecteur

Pendant les cinq minutes qui suivent le démarrage de la source, elle cherche votre lecteur habituel : l'instance enregistrée, sinon une autre instance du même lecteur (mpv relancé, par exemple), sinon un autre lecteur, celui qui joue de préférence. Vous avez ainsi le temps de lancer votre musique après OBS.

Passé ce délai, la source ne change plus d'elle-même : un autre lecteur qui démarre en plein direct ne prend jamais la place de celui qui est affiché. Si ce lecteur se ferme, la source affiche le message de repli et l'attend. Un lecteur choisi à la main dans les propriétés est définitif dès qu'il est choisi, et « Annuler » rétablit la sélection telle qu'elle était. Décocher « Sélection automatique » limite la source à l'instance enregistrée, et à elle seule.

## Vie privée

Par défaut, le plugin n'émet aucune requête réseau. La pochette en ligne se choisit source par source, parmi trois services :

- **MusicBrainz**, la base musicale libre, avec les images du Cover Art Archive. Le service reçoit le titre, l'artiste et l'album.
- **Wikipédia**, pour les disques connus. Le service reçoit l'artiste et l'album ou le titre.
- **YouTube**, la miniature de la vidéo, quand le lecteur donne l'adresse de la page (LibreWolf ou Firefox, pas Chrome). L'image est servie par Google.

La recherche n'a lieu que si le lecteur ne fournit pas de pochette lisible, une seule fois par piste et par service, échecs compris.

## Construire depuis les sources

Il faut Rust (la version est fixée par `rust-toolchain.toml`) et, pour les tests, `dbus-daemon`. Les bindings vers libobs sont générés à l'avance et versionnés : ni les en-têtes d'OBS ni libclang ne sont nécessaires.

```sh
git clone https://gitlab.com/iglou.eu/obs-dbus-mpris.git
cd obs-dbus-mpris
cargo test --workspace         # tests, chacun sur son propre bus D-Bus privé
scripts/install-dev.sh         # construit et installe dans OBS Flatpak
scripts/install-dev.sh --native   # ou dans un OBS natif
scripts/package-native.sh      # archive native de release, dans target/dist
scripts/build-flatpak.sh       # extension Flatpak hors ligne et son bundle
```

`install-dev.sh` refuse d'installer une bibliothèque qu'OBS ne pourrait pas charger (glibc trop récente, symbole libobs absent). Ne gardez jamais en même temps la copie de développement et l'extension Flatpak installée, le module serait chargé deux fois. `scripts/build-flatpak.sh` demande OBS Flatpak (branche stable), `org.freedesktop.Sdk//25.08` et son extension `rust-stable`.

L'exemple `cargo run --example watch` affiche dans le terminal ce que montrerait une source, sur vos vrais lecteurs, sans OBS.

## Organisation du code

- `crates/mpris-core/` : le cœur, sans OBS. Modèle de données, règles de sélection, adaptateur D-Bus, pochettes locales et en ligne.
- `crates/render/` : la disposition et le dessin de la source, sans OBS.
- `crates/obs-mpris/` : la bibliothèque chargée par OBS, et les traductions dans `data/locale/`.
- `crates/libobs-sys/` : les bindings vers libobs, régénérés par `scripts/gen-libobs-bindings.sh`.
- `packaging/flatpak/` : le manifeste de l'extension Flatpak.
- `docs/` : vision, besoins, architecture et feuille de route, en français.

## Licence et crédits

Le plugin est sous licence [GPL-3.0-or-later](LICENSE). Les pochettes en ligne viennent de [MusicBrainz](https://musicbrainz.org/) et du [Cover Art Archive](https://coverartarchive.org/), de [Wikipédia](https://www.wikipedia.org/) et de YouTube, et restent la propriété de leurs auteurs. Le texte est rendu par [cosmic-text](https://github.com/pop-os/cosmic-text). Le logo reprend l'esprit de celui d'OBS Studio, sans en être un dérivé officiel. OBS Studio est un projet indépendant de ce plugin.
