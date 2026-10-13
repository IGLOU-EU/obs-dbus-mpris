# Feuille de route

Les phases sont ordonnées par risque : ce qui peut faire échouer le projet passe en premier. Chaque phase se termine sur un état utilisable et testé, jamais sur un chantier ouvert.

La phase 2 a été avancée avant la phase 1 (décision du 2026-10-13) : le cœur ne dépend pas d'OBS et se teste seul.

Les validations se font dans OBS Flatpak. Le build natif est produit à chaque phase, mais sa validation dans un OBS natif est reportée.

## État d'avancement

| Phase | Objet | État |
|-------|-------|------|
| 0 | Cadrage | fait |
| 1 | Squelette marchant, natif et Flatpak | à faire |
| 2 | Cœur MPRIS, hors OBS | en cours : écrit et testé, reste la validation sur Chrome et Strawberry ou Elisa |
| 3 | Primitives de rendu | à faire |
| 4 | Source complète | à faire |
| 5 | Pochette en ligne, en option | à faire |
| 6 | Organisation visuelle | à faire |
| 7 | Distribution, version 1.0 | à faire |

## Phase 0. Cadrage

Ce dossier : vision, besoins, architecture, feuille de route.

**Fini quand** les quatre documents sont validés.

## Phase 1. Squelette marchant

Lever le risque principal, construire et charger une cdylib Rust dans OBS, avant d'écrire la moindre fonctionnalité.

- Workspace Cargo et ses trois crates (vides sauf le nécessaire).
- `libobs-sys` : génération par bindgen sur la liste blanche minimale, script de régénération, résultat commité (D-01).
- `obs-mpris` : déclaration de la source `mpris_now_playing`, qui dessine un rectangle de la couleur de fond, à la largeur et à la hauteur réglées.
- Frontière FFI étanche et journalisation via `blog` (D-07).
- Build natif et script d'installation dans `~/.config/obs-studio/plugins`, décision sur l'édition de liens avec `libobs.so`.
- Manifeste Flatpak, build hors ligne, installation locale de l'extension.

**Fini quand** la source apparaît et se règle dans OBS Flatpak 32.2, et que le build natif compile et s'installe.

Besoins couverts : B-31 (fond), B-33, B-40, B-42 (frontière FFI), B-44, B-47.

## Phase 2. Cœur MPRIS

Tout le modèle et toutes les règles, testés sans OBS.

- `Store`, `Snapshot` et adaptateur zbus : découverte, métadonnées, statut, `Seeked`, relecture périodique.
- Fonctions pures : clé de lecteur, règle de sélection, position extrapolée, format des temps, lecture tolérante des métadonnées.
- Décodage des miniatures `data:` et `file://` (D-03).
- Un exemple `cargo run --example watch` qui affiche en continu le lecteur effectif et son contenu, pour valider sur les vrais lecteurs (mpv, Chrome, Strawberry ou Elisa).

**Fini quand** les tests unitaires et d'intégration passent, et que `watch` donne un état juste sur les lecteurs réels.

Besoins couverts : B-01, B-04 à B-07, B-10 (décodage), B-11 à B-13, B-17, B-22, B-42.

## Phase 3. Primitives de rendu

Savoir dessiner chaque brique dans la source, avec un contenu encore fixe.

- Essai comparatif du rendu de texte et décision consignée dans [architecture.md](architecture.md) (D-04).
- Texture de texte, avec repli sur les caractères japonais.
- Texture de miniature depuis l'image décodée.
- Barre de progression (piste et remplissage).
- Cache de textures qui ne régénère que ce qui change (B-43).

**Fini quand** la source du squelette affiche un titre japonais, une miniature et une barre, aux couleurs et tailles réglées, sans débordement.

Besoins couverts : B-31, B-32, B-34, B-35, B-36, B-43.

## Phase 4. Source complète

Brancher le cœur sur la source et livrer tous les besoins fonctionnels de base, sur une mise en page provisoire et simple.

- Table unique des réglages (clés, types, valeurs par défaut) et propriétés OBS qui en découlent.
- Sélecteur de lecteur, entrée « (indisponible) », mémorisation et retour automatique.
- Les sept cases d'affichage, le flip, les états lecture, pause et arrêt, le message de repli.
- Traductions fr-FR et en-US.

**Fini quand** les scénarios d'acceptation 1 à 6 et 8 de [besoins.md](besoins.md) passent.

Besoins couverts : B-02, B-03, B-14 à B-16, B-20, B-21, B-23, B-30, B-46.

## Phase 5. Pochette en ligne, en option

Récupérer une pochette que le bac à sable Flatpak empêche de lire.

- Essai des services candidats sur des pistes réelles et choix consigné (D-09).
- Fil réseau, client HTTP (D-08), cache par piste avec échecs, dédoublonnage par le fil du cœur.
- Case `fetch_art_online`, décochée par défaut.

**Fini quand** le scénario d'acceptation 9 passe, et qu'aucune requête réseau ne part tant que l'option n'est cochée nulle part (B-48).

Besoins couverts : B-18, B-48.

## Phase 6. Organisation visuelle

La disposition laissée de côté au cadrage. Elle se définit ensemble avant d'être codée.

- Placement et alignement des éléments, marges.
- Disposition du sous-titre (artiste et album).
- Forme de l'indicateur de pause, rendu sans miniature.
- Textes trop longs : points de suspension ou défilement.

**Fini quand** le scénario d'acceptation 7 passe et que la disposition est validée à l'œil sur une vraie scène.

## Phase 7. Distribution

- Choix de la forge, intégration continue : tests, build natif, build Flatpak.
- Release : archive du `.so` avec ses traductions, bundle `.flatpak`.
- Documentation d'installation, override Flatpak compris (B-45).
- Versionnement sémantique et journal des changements.

**Fini quand** la version 1.0 est publiée et que tous les scénarios d'acceptation passent dans OBS Flatpak à partir des artefacts de la release.

## Plus tard, seulement si une friction réelle apparaît

Une décision remise à plus tard n'est pas perdue. Chacune de ces idées attend un besoin constaté.

- Validation du build natif dans un OBS natif.
- Miniatures `http(s)://` fournies directement par le lecteur, si un lecteur utilisé l'impose. Le coût devient faible une fois le client HTTP de la phase 5 en place.
- Choix de la police.
- Une couleur par élément de texte.
- Bouton « Rafraîchir » dans les propriétés, si la liste des lecteurs figée à l'ouverture gêne.
- Publication sur Flathub ou AUR.
