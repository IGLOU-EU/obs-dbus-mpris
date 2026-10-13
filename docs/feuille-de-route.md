# Feuille de route

Les phases sont ordonnées par risque : ce qui peut faire échouer le projet passe en premier. Chaque phase se termine sur un état utilisable et testé, jamais sur un chantier ouvert.

La phase 2 a été avancée avant la phase 1 (décision du 2026-10-13) : le cœur ne dépend pas d'OBS et se teste seul.

Les validations se font dans OBS Flatpak. Le build natif est produit à chaque phase, mais sa validation dans un OBS natif est reportée.

## État d'avancement

| Phase | Objet | État |
|-------|-------|------|
| 0 | Cadrage | fait |
| 1 | Squelette marchant, natif et Flatpak | fait, essai dans OBS en attente |
| 2 | Cœur MPRIS, hors OBS | fait, lecteur local (Strawberry ou Elisa) à vérifier à l'occasion |
| 3 | Rendu et disposition | fait, essai dans OBS en attente |
| 4 | Source complète | à faire |
| 5 | Pochette en ligne, en option | à faire |
| 6 | Organisation visuelle | à faire |
| 7 | Distribution, version 1.0 | à faire |

## Phase 0. Cadrage

Ce dossier : vision, besoins, architecture, feuille de route.

**Fini quand** les quatre documents sont validés.

## Phase 1. Squelette marchant

Lever le risque principal, construire et charger une cdylib Rust dans OBS, avant d'écrire la moindre fonctionnalité.

- `libobs-sys` : génération par bindgen sur la liste blanche minimale, script de régénération, résultat commité (D-01).
- `obs-mpris` : déclaration de la source `mpris_now_playing`, qui dessine un rectangle de la couleur de fond, à la largeur et à la hauteur réglées.
- Propriétés déclarées à partir de la table unique des réglages, avec leurs traductions fr-FR et en-US, et sélecteur de lecteur alimenté par le cœur, démarré au chargement du module.
- Frontière FFI étanche et journalisation via `blog` (D-07).
- Build natif et script d'installation de développement. OBS Flatpak charge les plugins du dossier utilisateur `~/.var/app/com.obsproject.Studio/config/obs-studio/plugins/`, ce qui évite de construire l'extension Flatpak à chaque essai. Le script refuse un `.so` qui exigerait une glibc plus récente que celle du runtime (2.42 pour la branche 25.08) ou un symbole absent de la `libobs` d'OBS.

**Fini quand** la source, installée par le script de développement, apparaît dans OBS Flatpak 32.2, se règle, dessine son fond et liste les lecteurs présents.

Besoins couverts : B-02, B-03 (sélecteur), B-31 (fond), B-33, B-40, B-42 (frontière FFI), B-44, B-46, B-47.

## Phase 2. Cœur MPRIS

Tout le modèle et toutes les règles, testés sans OBS.

- `Store`, `Snapshot` et adaptateur zbus : découverte, métadonnées, statut, `Seeked`, relecture périodique.
- Fonctions pures : clé de lecteur, règle de sélection, position extrapolée, format des temps, lecture tolérante des métadonnées.
- Décodage des miniatures `data:` et `file://` (D-03).
- Un exemple `cargo run --example watch` qui affiche en continu le lecteur effectif et son contenu, pour valider sur les vrais lecteurs (mpv, Chrome, Strawberry ou Elisa).

**Fini quand** les tests unitaires et d'intégration passent, et que `watch` donne un état juste sur les lecteurs réels.

Validé le 2026-10-13 sur mpv (pochette en `data:`, album japonais) et sur LibreWolf avec YouTube, qui tient lieu de navigateur : aucune pochette, `mpris:trackid` constant, durée et position publiées seulement une fois que la page les fournit (avant, la durée est inconnue et la position reste à 0).

Puis avec quatre lecteurs en même temps, tous en lecture : AudioTube (pochette WebP), Chrome (artiste vide, pochette dans son `/tmp` privé), LibreWolf et VLC (pochette JPEG en 16:9, durée au-delà d'une heure). Sans lecteur mémorisé, la règle B-05 choisit bien le premier en lecture par ordre alphabétique. Cinq flux ouverts n'ont donné que quatre lecteurs MPRIS : une application qui joue plusieurs flux n'en expose apparemment qu'un.

Enfin, trois instances de mpv en lecture, chacune avec sa pochette en `data:` : elles partageaient une seule entrée `mpv` dans le sélecteur, et la source suivait l'une d'elles, arbitrairement. Plusieurs mpv en même temps étant un usage réel, le sélecteur propose désormais une entrée par instance, libellée par son titre (B-02, B-03, B-05). FreeTube n'expose pas MPRIS et n'apparaît donc pas, ce qui reste hors périmètre.

Besoins couverts : B-01, B-04 à B-07, B-10 (décodage), B-11 à B-13, B-17, B-22, B-42.

## Phase 3. Rendu et disposition

Dessiner les quatre blocs selon la maquette de B-37, à partir du contenu réel du lecteur suivi. La disposition, prévue au départ en phase 6, a été fixée le 2026-10-14 et rejoint cette phase.

- Essai du rendu de texte dans le bac à sable d'OBS Flatpak, avec repli sur les caractères japonais, et décision consignée (D-04).
- Calcul pur de la disposition à partir des réglages, puis dessin de la source entière en une image, testés hors d'OBS.
- Miniature recadrée au carré, barre, temps, pictogramme pause, message de repli, coins arrondis, flip.
- Les sept cases d'affichage et les états lecture, pause et arrêt, à partir du lecteur que la source suit.
- Dessin sur un fil dédié, qui charge aussi les polices, et image montrée par la source comme une texture.
- Image régénérée seulement quand ce qu'elle montre change (B-43).

**Fini quand** la source affiche dans OBS la piste suivie selon la maquette, japonais compris, sans débordement, et suit chaque réglage.

Rendu vérifié hors d'OBS le 2026-10-14 par l'exemple `render_png`, sur l'hôte et dans le bac à sable d'OBS Flatpak (D-04). L'essai dans OBS reste à faire.

Besoins couverts : B-10, B-14 à B-16, B-20 à B-23, B-30 à B-38, B-43.

## Phase 4. Source complète

Ce que cette phase devait livrer l'a été plus tôt : le sélecteur et les traductions fr-FR et en-US en phase 1, le branchement du cœur sur la source, les cases d'affichage, le flip, les états lecture, pause et arrêt et le message de repli en phase 3. Reste à vérifier dans OBS Flatpak le comportement de la source dans la durée, sur des lecteurs réels.

- Scénarios d'acceptation 1 à 6 et 8 joués dans OBS Flatpak, et correction de ce qu'ils révèlent.

**Fini quand** les scénarios d'acceptation 1 à 6 et 8 de [besoins.md](besoins.md) passent.

Besoins couverts : aucun nouveau, la phase valide dans OBS ceux des phases 1 à 3.

## Phase 5. Pochette en ligne, en option

Récupérer une pochette que le bac à sable Flatpak empêche de lire.

- Essai des services candidats sur des pistes réelles et choix consigné (D-09).
- Fil réseau, client HTTP (D-08), cache par piste avec échecs, dédoublonnage par le fil du cœur.
- Case `fetch_art_online`, décochée par défaut.

**Fini quand** le scénario d'acceptation 9 passe, et qu'aucune requête réseau ne part tant que l'option n'est cochée nulle part (B-48).

Besoins couverts : B-18, B-48.

## Phase 6. Organisation visuelle

Retouches de la disposition livrée en phase 3, une fois vue sur une vraie scène : proportions des marges, défilement des textes longs si les points de suspension gênent.

**Fini quand** la disposition est validée à l'œil sur une vraie scène.

## Phase 7. Distribution

- Manifeste de l'extension Flatpak et build hors ligne (pendant le développement, le `.so` natif suffit).
- Choix de la forge, intégration continue : tests, build natif, build Flatpak.
- Release : archive du `.so` avec ses traductions, bundle `.flatpak`.
- Documentation d'installation, override Flatpak (B-45) et police CJK sur l'hôte compris, puisque le runtime n'en fournit aucune (D-04).
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
