# Architecture

## Principes

### Single point of truth

Chaque donnée a un seul propriétaire et un seul endroit où elle s'écrit. Tout ce qui peut se dériver est dérivé à la demande, jamais stocké en double.

| Donnée | Propriétaire unique | Écrite par | Lue par |
|--------|---------------------|------------|---------|
| État des lecteurs (métadonnées, statut, position de référence, miniature décodée) | le `Store` du cœur MPRIS | le fil du cœur, et lui seul | les sources, via un instantané immuable |
| Pochettes trouvées en ligne (cache par piste, échecs compris) | le `Store` du cœur MPRIS | le fil du cœur, à réception des résultats du fil réseau | les sources, via l'instantané |
| Réglages d'une source | les `obs_data` d'OBS, persistés avec la collection de scènes | OBS, depuis les propriétés | la source, convertis en une structure `Settings` immuable à chaque `update` |
| Schéma des réglages (clés, types, valeurs par défaut) | une table unique dans le code | | la déclaration des propriétés, les valeurs par défaut, la conversion en `Settings` |
| Lecteur effectif | personne | jamais stocké, recalculé à chaque image par une fonction pure | |
| Position affichée | personne | jamais stockée, dérivée de la position de référence, de son horodatage, de `Rate` et de l'heure courante | |
| Textures | le cache de rendu de chaque source | régénérées quand leurs entrées changent | le rendu |

Une miniature décodée vit une seule fois dans le `Store`, partagée par toutes les sources qui suivent le même lecteur.

### Data oriented

- Des structures de données simples, sans hiérarchie d'objets ni trait object pour le domaine. Les lecteurs sont désignés par leur clé, pas par des pointeurs.
- La logique s'écrit en fonctions pures qui transforment des données : `select_player(&Snapshot, &Settings)`, `display_position(&Player, now)`, `format_time(Duration)`, `layout(&Settings, &Content)`.
- Les effets de bord restent aux bords : l'adaptateur D-Bus en entrée, la colle OBS en sortie. Le cœur se teste sans OBS et sans bus.

## Flux de données

```
bus de session D-Bus
  │  NameOwnerChanged, PropertiesChanged, Seeked, lectures Get
  ▼
fil du cœur (zbus) ──► Store ──► publie un Arc<Snapshot> immuable
                                         │
           ┌─────────────────────────────┘
           ▼
video_tick, par source (fil graphique d'OBS)
  ├─ lit le dernier Snapshot
  ├─ select_player(snapshot, settings)         fonction pure
  ├─ content(player, now) → Content            fonction pure : textes, temps, état
  └─ si Content ou Settings a changé : régénère les textures concernées
           ▼
video_render : dessine fond, miniature, textes, barre (aucune entrée-sortie)
```

Le `Store` indexe les lecteurs par nom connu (`org.mpris.MediaPlayer2.mpv.instance-…`) et rattache les signaux par nom unique D-Bus (`:1.42`), puisque c'est de ce nom qu'ils arrivent. Un même processus peut posséder plusieurs noms MPRIS, un signal touche alors chacun d'eux.

Pochette en ligne (B-18), quand une source a l'option cochée et que la miniature locale manque :

```
source ── demande ──► fil du cœur : déjà en cache ou en cours ? sinon ──► fil réseau
                                                                            │
fil du cœur ◄── résultat : pochette décodée, ou échec ───────────────────────┘
  └─ met à jour le Store, publie un nouvel instantané
```

Le fil du cœur reste le seul écrivain du `Store` et le seul à dédoublonner les demandes, quel que soit le nombre de sources qui demandent la même pochette.

## Découpage

Un workspace Cargo de trois crates :

```
crates/
  libobs-sys/   bindings FFI bruts vers libobs, générés par bindgen et commités
  mpris-core/   modèle, règles pures, adaptateur D-Bus. Aucune dépendance à OBS.
  obs-mpris/    cdylib chargée par OBS : déclaration de la source, propriétés, rendu
```

La séparation entre `mpris-core` et `obs-mpris` a une raison concrète : les tests du cœur doivent tourner avec `cargo test`, sans `libobs` à l'édition de liens. Dans `mpris-core`, le module D-Bus est isolé, les modules purs ne l'importent pas.

## Fils d'exécution

- **Fil du cœur** : un seul pour tout le plugin, démarré par `obs_module_load` et arrêté par `obs_module_unload`. Une boucle d'événements asynchrone unique qui reçoit les signaux D-Bus, le minuteur de relecture et les messages des autres fils. L'exécuteur de zbus tourne dans ce fil (`internal_executor(false)`), ce qui permet à l'arrêt d'interrompre même un appel en attente sur un lecteur figé. Les miniatures se lisent et se décodent sur le pool de `blocking`, attendues dans cette même boucle, si bien que l'arrêt abandonne aussi une lecture bloquée sur un montage réseau figé. Ce pool, par lequel zbus se connecte aussi, et le fil global `async-io` sont partagés par tout le processus et survivent à l'arrêt du cœur, sans aucun état de lecteur. Un bus injoignable n'empêche pas le chargement du module, il est journalisé et la source affiche le message de repli.
- **Fil réseau** : démarré à la première demande de pochette en ligne, jamais sinon (B-48). Il ne touche pas au `Store`, il renvoie ses résultats au fil du cœur.
- **Fil graphique d'OBS** (`video_tick`, `video_render`, et `update`, qu'OBS 32 diffère au tick graphique pour une source vidéo) : lit l'instantané courant, sans attente sur le fil du cœur.
- **Fil d'interface d'OBS** (`create`, `get_defaults`, `get_properties`) : lit l'instantané pour construire la liste des lecteurs.

Les réglages d'une source sont écrits et lus depuis ces deux fils : ils sont protégés par un `Mutex`, tenu le temps d'une copie.

L'instantané est le seul état partagé entre fils. Il se publie par remplacement atomique d'un `Arc` (`arc-swap` ou un `Mutex<Arc<Snapshot>>` tenu le temps d'un clonage).

## Décisions techniques

**D-01 Bindings libobs maison.** Le crate `obs-wrapper` n'est plus maintenu (dernière version 0.4.1, septembre 2022). Les bindings sont générés par bindgen à partir des en-têtes d'OBS 32, présents dans OBS Flatpak (`/app/include/obs`) et dans le paquet `obs-studio` natif. On limite la génération à l'API utilisée (liste blanche) et on commite le résultat, pour que le build n'exige ni libclang ni en-têtes. Un script les régénère au changement de version majeure d'OBS : `scripts/gen-libobs-bindings.sh [include-dir]`, qui pilote le générateur `tools/gen-libobs-bindings` (projet cargo exclu du workspace, seul à dépendre de bindgen). La liste blanche nomme chaque fonction utilisée, garde opaques les types qu'`obs_source_info` référence sans que le plugin s'en serve, et génère les enums en constantes, pour qu'une valeur inconnue venue d'OBS ne soit jamais un comportement indéfini. La disposition des structures est vérifiée à la compilation.

**D-02 D-Bus avec zbus 5.** Pur Rust, maintenu, sans dépendance à libdbus. On n'utilise pas le crate `mpris`, qui passe par libdbus et n'apporte rien pour deux interfaces en lecture seule. Chaque appel de méthode expire après une seconde, et une erreur d'appel vaut donnée manquante : un lecteur figé ne bloque jamais les autres. La journalisation passe par la façade `log`, que la colle OBS relaiera vers `blog`.

**D-03 Images par le crate `image`**, limité aux formats rencontrés chez les lecteurs réels : JPEG, PNG, et WebP (AudioTube). Un seul chemin pour `data:`, `file://` et les pochettes en ligne : des octets, décodés en RGBA hors du fil graphique, puis envoyés en texture (`gs_texture_create`) par le fil graphique. Un chargement qui dépasse deux secondes (fichier sur un montage réseau figé) est abandonné et compte comme un échec, pour que l'état des autres lecteurs continue d'avancer. Écarté : `gs_image_file`, qui n'accepte qu'un chemin de fichier et forcerait à écrire les miniatures `data:` sur disque.

**D-04 Rendu du texte : rastérisation en Rust par `cosmic-text` 0.17**, tranchée par l'essai de la phase 3 (2026-10-14). La source entière, texte compris, est dessinée en une image RGBA par le crate `render`, puis montrée comme une texture. Options comparées :

| Option | Pour | Contre |
|--------|------|--------|
| Sources texte privées d'OBS (`text_ft2_source_v2`) en sources enfants | aucun rastériseur à maintenir, polices système d'OBS | contrôle limité sur la troncature et l'alignement, dépend du comportement d'une autre source |
| Rastérisation en Rust (`cosmic-text`, mise en forme et repli de polices) vers une texture | contrôle total, même rendu en natif et en Flatpak | dépendance lourde, découverte des polices à gérer |

Critères de choix : repli correct sur les caractères japonais (cas réel de mpv), troncature dans une largeur donnée, alpha du texte, coût CPU. L'option `cosmic-text` les remplit tous, l'autre n'a pas été essayée. Constats :

- **Japonais.** « 愛をとりもどせ!! (MOVIE ver.) » s'affiche sur l'hôte et dans le bac à sable d'OBS Flatpak 32.2.2, avec des images identiques au pixel près : le bac à sable voit les polices de l'hôte sous `/run/host/fonts`, Noto Sans CJK et Noto Color Emoji compris. Les idéogrammes suivent la langue du système (Noto Sans CJK SC hors d'une locale japonaise), les kanas Noto Sans CJK JP.
- **Police par défaut.** C'est celle de `cosmic-text` (Open Sans, puis Noto Sans, DejaVu Sans, FreeSans), et non celle de fontconfig : `fontdb` retient le dernier alias `sans-serif` qu'il lit, Nimbus Sans Narrow sur l'hôte et FreeSans dans le bac à sable, là où `fc-match` donne Noto Sans et DejaVu Sans. Suivre exactement fontconfig demanderait de lier libfontconfig ou de refaire son ordre de préférence.
- **Troncature.** Coupure sur les avances des glyphes mis en forme, vérifiée par une seconde mise en forme, points de suspension compris dans la largeur.
- **Alpha.** La couverture des glyphes se mélange en lumière linéaire dans l'image prémultipliée, comme le reste de la source.
- **Coût.** La lecture des polices du système prend 80 ms à chaud et 2,1 s à froid (2 868 faces sur le poste de développement). Un redessin du panneau complet avec miniature (600 × 156) prend 0,55 ms, mais le premier d'une piste monte à 3,6 ms (pochette 360 × 360) et 12 ms (1024 × 1024, le maximum gardé par le cœur), et le premier usage d'une police de repli coûte quelques millisecondes. Le dessin tourne donc sur un fil dédié, qui charge aussi les polices, et le fil graphique ne fait que demander des images et envoyer la dernière reçue en texture.
- **glibc.** Le rastériseur de swash (`zeno`) appelle `acosf`, qu'un hôte en glibc 2.43 ou plus lie en version 2.43, absente du runtime 25.08 (2.42). La bibliothèque l'enveloppe (`-Wl,--wrap=acosf`) et le remplace par celui du crate `libm` : elle n'exige pas plus que GLIBC_2.39, comme avant `cosmic-text`.

**D-05 Position extrapolée** (B-17). Recalage sur `Seeked`, sur changement de piste ou d'état, et relecture périodique de `Position` à un intervalle à régler à l'usage, pour les lecteurs qui omettent `Seeked`.

**D-06 Choix mémorisé et clé de lecteur.** Le réglage mémorise le nom de bus de l'instance choisie (B-03). La clé de lecteur (B-04) en est dérivée à la demande, jamais stockée en double. Ordre « premier » : alphabétique sur la clé, puis sur le nom de bus, sans tenir compte de la casse (B-05).

**D-07 Frontière FFI étanche.** Chaque rappel `extern "C"` passe par `catch_unwind`. Les erreurs sont journalisées via `blog`, jamais propagées à OBS.

**D-08 Client HTTP `ureq`**, bloquant, léger, TLS par rustls, utilisé seulement dans le fil réseau avec un délai d'expiration court. Pas de runtime asynchrone ajouté pour quelques requêtes.

**D-09 Service de pochettes : à trancher par un essai au moment d'implémenter B-18.** Candidats sans clé d'API :

| Service | Particularités |
|---------|----------------|
| API Deezer | une requête de recherche, pochette fournie dans la réponse |
| iTunes Search API | une requête de recherche, pochette redimensionnable par l'URL |
| MusicBrainz et Cover Art Archive | données ouvertes, deux requêtes, une requête par seconde au plus, User-Agent identifiable exigé |

Critères de choix : couverture sur des pistes réelles (japonaises comprises), justesse du résultat, conditions d'utilisation compatibles avec un plugin distribué.

## Builds

**Natif.** `cargo build --release` produit la cdylib, installée sous le nom `obs-dbus-mpris.so` :

```
~/.config/obs-studio/plugins/obs-dbus-mpris/
  bin/64bit/obs-dbus-mpris.so
  data/locale/fr-FR.ini, en-US.ini
```

Le plugin ne lie pas `libobs` à l'édition de liens : ses symboles se résolvent au chargement, dans le processus d'OBS qui a déjà chargé `libobs`. Le build n'a ainsi besoin ni de `libobs` ni de ses en-têtes, seulement des bindings commités (D-01). rustc ne passe pas `-z defs` et active `BIND_NOW` : tout se résout au `dlopen`, qu'OBS fait en `RTLD_NOW`. La cdylib `libobs_mpris.so` est installée sous le nom `obs-dbus-mpris.so`.

La clé de traduction d'une propriété est sa clé de réglage, et un test vérifie que chaque fichier de langue contient exactement les clés utilisées.

**Développement avec OBS Flatpak.** `scripts/install-dev.sh [--native]` construit le plugin en release et le copie (par défaut dans le dossier Flatpak, avec `--native` dans `~/.config/obs-studio/plugins`). Le même `.so` natif est copié dans `~/.var/app/com.obsproject.Studio/config/obs-studio/plugins/obs-dbus-mpris/` (mêmes sous-dossiers `bin/64bit` et `data/locale`), qu'OBS Flatpak charge directement. Il tourne alors avec la glibc du runtime (2.42), plus ancienne que celle de l'hôte : le script d'installation vérifie qu'aucun symbole plus récent n'est requis, et que chaque symbole `libobs` attendu existe dans la `libobs` d'OBS Flatpak.

**Flatpak.** Un manifeste `com.obsproject.Studio.Plugin.DbusMpris` construit une extension de l'application OBS. Disposition relevée sur l'extension Tuna installée et dans les métadonnées d'OBS (`directory=plugins`, `merge-dirs=lib/obs-plugins;share/obs/obs-plugins`) :

```
runtime          com.obsproject.Studio, branche stable
sdk              org.freedesktop.Sdk//25.08
sdk-extensions   org.freedesktop.Sdk.Extension.rust-stable
build-extension  true
préfixe          /app/plugins/DbusMpris
fichiers         lib/obs-plugins/obs-dbus-mpris.so
                 share/obs/obs-plugins/obs-dbus-mpris/locale/*.ini
```

Le build Flatpak se fait hors ligne : les dépendances Cargo sont fournies au manifeste par `flatpak-cargo-generator`. La permission D-Bus passe par un override (B-45). L'accès réseau nécessaire à B-18 est déjà accordé à OBS Flatpak (`shared=network`).

## Tests

- **Unitaires, test d'abord, dans `mpris-core`** : règle de sélection (dont lecteur mémorisé présent mais arrêté), extraction de la clé, extrapolation de la position, lecture des métadonnées (manquantes, mal typées), décodage des miniatures (`data:`, `file://`, format non pris en charge, données corrompues), format des temps, conversion des réglages et valeurs par défaut, mise en page (aucun débordement, flip), pochette en ligne (construction de la recherche, lecture des réponses du service à partir de réponses enregistrées, cache et dédoublonnage, échecs compris). Aucun test n'appelle le vrai service.
- **Intégration D-Bus** : un faux lecteur MPRIS servi par zbus sur un bus privé (`dbus-run-session -- cargo test`). Scénarios : apparition, disparition, changement de métadonnées, `Seeked`, relance avec un nouveau suffixe d'instance.
- **Colle OBS** : scénarios d'acceptation manuels ([besoins.md](besoins.md), section 7) dans OBS Flatpak.

## Risques

| Risque | Traitement |
|--------|------------|
| Build Rust hors ligne en Flatpak | reporté en phase 7 avec le manifeste, le développement passant par le dossier de plugins utilisateur |
| Rendu du texte et repli CJK | essai comparatif en phase 3 (D-04) |
| Miniatures hors du bac à sable d'OBS Flatpak (Chrome en Flatpak) | option de pochette en ligne (B-18) |
| Pochette en ligne fausse ou introuvable (titres de vidéos non normalisés) | accepté : l'option est décochée par défaut, un échec laisse l'emplacement vide |
| Build natif jamais chargé dans un OBS natif | accepté pour l'instant, validation reportée |
| Écarts d'ABI de libobs entre versions mineures | bindings générés depuis OBS 32, testés sur 32.x uniquement |
| Lecteurs qui omettent `Seeked` ou renvoient une position fausse | relecture périodique (D-05) |
