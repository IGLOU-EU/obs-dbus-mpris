# Architecture

## Principes

### Single point of truth

Chaque donnée a un seul propriétaire et un seul endroit où elle s'écrit. Tout ce qui peut se dériver est dérivé à la demande, jamais stocké en double.

| Donnée | Propriétaire unique | Écrite par | Lue par |
|--------|---------------------|------------|---------|
| État des lecteurs (métadonnées, statut, position de référence, miniature décodée) | le `Store` du cœur MPRIS | le fil du cœur, et lui seul | les sources, via un instantané immuable |
| Pochettes trouvées en ligne (cache de la session par service et par piste, échecs compris) | le `Store` du cœur MPRIS, qui garde chaque pochette telle que téléchargée, et décodée seulement dans les lecteurs dont la piste affichée est la sienne | le fil du cœur, à réception des résultats du fil réseau | les sources, via l'instantané |
| Vœu de pochette en ligne d'une source (service choisi, lecteur affiché) | la source, qui le tient dans son `ArtWish` | le fil graphique : `video_tick` le recalcule à chaque image et ne l'envoie au fil du cœur que quand il change, la fin de la source le retire | le fil du cœur, qui garde le dernier vœu de chaque source |
| Réglages d'une source | les `obs_data` d'OBS, persistés avec la collection de scènes | OBS, depuis les propriétés | la source, convertis en une structure `Settings` immuable à chaque `update` |
| Schéma des réglages (clés, groupes, types, valeurs par défaut, présence d'un texte d'aide) | une table unique dans le code | | la déclaration des propriétés, de leurs groupes et de leurs textes d'aide, les valeurs par défaut, la conversion en `Settings` |
| Lecteur effectif | personne | jamais stocké, recalculé à chaque image par une fonction pure, d'après l'instantané, les réglages, le lecteur suivi et la phase de la source | |
| Lecteur suivi d'une source (B-08) | la source, en mémoire seulement, jamais dans ses réglages | le fil graphique : `video_tick` y met le lecteur que la sélection rend, `update` y met l'instance choisie à la main, ou rien pour « Aucun choix », quand le lecteur mémorisé ou la sélection automatique changent, ou rétablit celui d'avant la modification quand ce choix revient à sa valeur d'avant (Annuler) | la sélection de l'image suivante |
| Instant de démarrage et choix manuel d'une source (B-08) | la source, en mémoire seulement, jamais dans ses réglages, si bien que chaque lancement d'OBS rouvre le délai de grâce | `create` note l'instant de démarrage, `update` note le choix manuel quand le lecteur mémorisé ou la sélection automatique changent, ou rétablit celui d'avant la modification (Annuler) | le calcul de la phase |
| État d'avant la modification du choix (pour Annuler, B-05) | la source, en mémoire seulement, jamais dans ses réglages | `update` : le premier changement du lecteur mémorisé ou de la sélection automatique y note le choix d'avant, avec le lecteur suivi et le choix manuel d'alors, les changements suivants le gardent, et le retour à ce choix le rétablit puis l'efface. Le fil d'interface : `get_properties` l'efface à chaque ouverture des propriétés, pour qu'un retour à l'ancien lecteur dans un dialogue ultérieur reste un choix manuel | `update` |
| Phase d'une source (délai de grâce ou figée, B-05) | personne | jamais stockée, dérivée à chaque image par une fonction pure de l'instant de démarrage, du choix manuel et de l'heure : figée dès 5 minutes (constante `GRACE` du cœur) ou dès un choix manuel | |
| Position affichée | personne | jamais stockée, dérivée de la position de référence, de son horodatage, de `Rate` et de l'heure courante | |
| Image d'une source | la source, qui en garde une texture pour l'image et une par texte qui défile | le fil de rendu, qui dessine la source entière en une image quand sa clé de redessin change, les textes trop longs à part dans des bandes, puis le fil graphique, qui charge l'image en texture, et les bandes seulement quand elles changent | `video_render`, qui dessine l'image puis une fenêtre de chaque bande |
| Décalage d'un texte qui défile | personne | jamais stocké, dérivé à chaque image du temps écoulé depuis le début du cycle, de la vitesse, de la pause et des périodes des bandes | |

Une miniature décodée vit une seule fois dans le `Store`, partagée par toutes les sources qui suivent le même lecteur. Chaque source n'en garde que sa version mise à l'échelle, dérivée et refaite seulement quand l'image, le côté de l'emplacement ou le rayon des coins change.

### Data oriented

- Des structures de données simples, sans hiérarchie d'objets ni trait object pour le domaine. Les lecteurs sont désignés par leur clé, pas par des pointeurs.
- La logique s'écrit en fonctions pures qui transforment des données : `phase(démarrage, choix manuel, now)`, `select(&[Player], &Settings, suivi, phase)`, `rechoose(état, annulation, avant, après)`, `display_position(&Player, now)`, `format_time(Duration)`, `layout(&Settings)`, `redraw_key(&Layout, &Content, &Settings)`, `Scroll::offset(elapsed, period)`.
- Les effets de bord restent aux bords : l'adaptateur D-Bus en entrée, la colle OBS en sortie. Le cœur se teste sans OBS et sans bus, le rendu sans OBS.

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
  ├─ phase(démarrage, choix manuel, now)         fonction pure : délai de grâce ou figée
  ├─ select(snapshot, settings, suivi, phase)    fonction pure : lecteur affiché, lecteur suivi
  ├─ garde le lecteur suivi pour l'image suivante
  ├─ content(player, settings, now) → Content    fonction pure : textes, temps, état
  ├─ layout(settings) → Layout                   fonction pure : taille et rectangles
  ├─ redraw_key(layout, content, settings)       fonction pure : ce que l'image montre
  ├─ si la clé a changé : demande une image au fil de rendu, sans l'attendre
  └─ si la vitesse ou la pause du défilement a changé : relance le cycle
           ▼
fil de rendu mpris-render, un seul pour tout le plugin
  └─ draw(layout, content, settings) → Picture
       ├─ image RGBA de la source entière, sans les textes trop longs
       └─ une bande par texte trop long (B-36), dessinée une fois
          et rendue telle quelle tant qu'elle montre la même chose
     (une demande plus récente remplace celle qui attend encore)
           ▼
video_render, par source (fil graphique d'OBS)
  ├─ prend la dernière image finie, s'il y en a une
  ├─ la charge en texture, à la place de la précédente
  ├─ charge les bandes en textures si elles ont changé, et relance alors le cycle
  ├─ dessine la texture de l'image
  └─ pour chaque bande : Scroll::offset(écoulé, période)   fonction pure
     puis dessine la fenêtre de la bande à ce décalage, sur sa ligne
```

Le `Store` indexe les lecteurs par nom connu (`org.mpris.MediaPlayer2.mpv.instance-…`) et rattache les signaux par nom unique D-Bus (`:1.42`), puisque c'est de ce nom qu'ils arrivent. Un même processus peut posséder plusieurs noms MPRIS, un signal touche alors chacun d'eux.

Pochette en ligne (B-18), quand une source a choisi un service :

```
video_tick, par source
  └─ vœu : (service choisi, lecteur affiché), ou aucun     fonction pure online::wish
     envoyé par un canal, seulement quand il change
           ▼
fil du cœur, à chaque vœu et à chaque changement publié
  ├─ Store::start_lookups(vœux)
  │    pour chaque vœu : miniature locale absente ou illisible ?
  │    requête de la piste pour ce service (nettoyage, identifiant de vidéo) ?
  │    jamais demandée dans la session ? alors marquée demandée
  └─ envoie chaque requête nouvelle au fil réseau, démarré à la première
           ▼
fil réseau (mpris-online), une requête à la fois
  └─ recherches puis image, rythmées par hôte          online::look_up, pur, sur un fetch
     renvoie au fil du cœur les octets de la pochette, ou rien
           ▼
fil du cœur
  ├─ Store : la requête trouvée (octets gardés) ou manquée, pour la session
  ├─ décode sur le pool blocking les pochettes que les pistes affichées réclament
  │    (première fois, ou retour à une piste déjà trouvée), deux secondes au plus
  └─ Store : l'image décodée rejoint les lecteurs dont la piste est la sienne,
     puis nouvel instantané
           ▼
content(), par source : la pochette du lecteur d'abord, sinon celle que le service choisi a trouvée
```

Le fil du cœur reste le seul écrivain du `Store` et le seul à dédoublonner les demandes, quel que soit le nombre de sources qui demandent la même pochette. Il est aussi le seul à savoir quand une recherche est utile, puisqu'il voit la miniature locale de chaque lecteur et le cache de la session. Une source ne sait que ce qu'elle montre : elle envoie donc son vœu, le service qu'elle a choisi pour le lecteur qu'elle affiche, et seulement quand il change. C'est le plus petit lien qui laisse la décision au fil du cœur sans qu'il interroge les sources. Deux sources qui suivent la même piste avec des services différents donnent deux recherches, une par service, et chacune voit la pochette de son service.

Le cache garde chaque pochette trouvée telle que téléchargée (50 à 100 Ko à 500 pixels), pas décodée (1 Mo), comme l'étude le recommande : sur une longue session, seuls les lecteurs dont la piste affichée a une pochette trouvée en tiennent l'image décodée. Revenir à une piste déjà trouvée la décode à nouveau, sans requête réseau.

## Découpage

Un workspace Cargo de quatre crates :

```
crates/
  libobs-sys/   bindings FFI bruts vers libobs, générés par bindgen et commités
  mpris-core/   modèle, règles pures, adaptateur D-Bus, fil réseau. Aucune dépendance à OBS.
  render/       disposition, dessin de la source en une image RGBA, fil de rendu. Aucune dépendance à OBS.
  obs-mpris/    cdylib chargée par OBS : déclaration de la source, propriétés, image montrée en texture
```

La séparation entre `mpris-core` et `obs-mpris` a une raison concrète : les tests du cœur doivent tourner avec `cargo test`, sans `libobs` à l'édition de liens. Dans `mpris-core`, le module D-Bus est isolé, les modules purs ne l'importent pas. Le crate `render` obéit à la même règle : il lit les réglages et le contenu du cœur, dessine avec `cosmic-text` pour le texte, et tout son dessin se teste avec `cargo test`.

## Fils d'exécution

- **Fil du cœur** : un seul pour tout le plugin, démarré par `obs_module_load` et arrêté par `obs_module_unload`. Une boucle d'événements asynchrone unique qui reçoit les signaux D-Bus, le minuteur de relecture et les messages des autres fils. L'exécuteur de zbus tourne dans ce fil (`internal_executor(false)`), ce qui permet à l'arrêt d'interrompre même un appel en attente sur un lecteur figé. Les miniatures se lisent et se décodent sur le pool de `blocking`, attendues dans cette même boucle, si bien que l'arrêt abandonne aussi une lecture bloquée sur un montage réseau figé. Ce pool, par lequel zbus se connecte aussi, et le fil global `async-io` sont partagés par tout le processus et survivent à l'arrêt du cœur, sans aucun état de lecteur. Un bus injoignable n'empêche pas le chargement du module, il est journalisé et la source affiche le message de repli.
- **Fil réseau** (`mpris-online`) : un seul pour tout le plugin, démarré par le fil du cœur à la première recherche de pochette en ligne, jamais sinon (B-48). Il ne touche pas au `Store` : il reçoit les requêtes du fil du cœur par un canal, les traite une à une, et lui renvoie par un autre canal les octets de chaque pochette trouvée, ou rien, que la boucle du cœur applique comme un événement. Toutes les requêtes du plugin passant par lui, l'espacement d'au moins 1,1 s entre deux requêtes au même hôte vaut pour tout le plugin, ce qui tient la limite d'une requête par seconde de MusicBrainz. Une panique dans une recherche est rattrapée et compte comme un échec (B-42). L'arrêt du cœur ne l'attend pas : le canal des requêtes se ferme et un drapeau d'arrêt interrompt les attentes, si bien qu'il s'arrête à sa prochaine vérification, au pire à la fin de sa requête en cours, bornée à 5 s. Il ne garde aucun état de lecteur.
- **Fil de rendu** (`mpris-render`) : un seul pour tout le plugin, démarré par `obs_module_load` et arrêté par `obs_module_unload`, qui attend la fin de l'image ou du chargement de polices en cours. Il charge d'abord les polices du système, ce qui prend jusqu'à quelques secondes à froid, puis dessine les images que les sources demandent. Pour chaque source, il ne garde qu'une demande en attente, qu'une plus récente remplace, et qu'une image finie, que le fil graphique vient prendre. Un dessin qui panique est journalisé sans arrêter le fil, un échec du chargement des polices laisse le texte non dessiné, et un fil qui ne démarre pas laisse les sources vides sans empêcher le chargement du module.
- **Fil graphique d'OBS** (`video_tick`, `video_render`, et `update`, qu'OBS 32 diffère au tick graphique pour une source vidéo) : lit l'instantané courant sans attendre le fil du cœur, demande une image au fil de rendu sans l'attendre, et charge en texture la dernière image finie et ses bandes nouvelles. À chaque image, il ne fait que calculer le décalage de chaque bande et la dessiner : le défilement ne redessine rien sur le processeur. Il tient aussi le lecteur suivi de chaque source (B-08), avec son instant de démarrage, le fait qu'un choix manuel a eu lieu et l'état d'avant la dernière modification du choix, qu'il range avec l'état d'affichage de la source, derrière le même `Mutex` : `video_tick` en dérive la phase et remplace le lecteur suivi par celui que rend la sélection, et `update`, qui tourne sur ce même fil, passe l'ancien et le nouveau réglage à `rechoose`. Celle-ci en fait un choix manuel, l'instance choisie ou rien pour « Aucun choix », sauf quand le choix revient à sa valeur d'avant la modification, ce qu'OBS fait sur Annuler : l'état d'avant revient alors, délai de grâce compris. L'un et l'autre prennent ce verrou après avoir relâché celui des réglages, jamais l'un dans l'autre.
- **Fil d'interface d'OBS** (`create`, `get_defaults`, `get_properties`) : lit l'instantané pour construire la liste des lecteurs. `create` note aussi l'instant de démarrage de la source, qui ouvre son délai de grâce (B-05), avant que la source ne soit visible des autres fils. `get_properties` efface l'état d'avant la modification du choix, derrière le verrou de l'état d'affichage, jamais tenu en même temps que celui des réglages : ce qu'Annuler rétablit ne vaut que jusqu'à la prochaine ouverture des propriétés. « Par défaut » recharge aussi les propriétés, si bien que « Par défaut » puis « Annuler » compte comme un choix manuel du lecteur d'origine, limite acceptée.

Les réglages d'une source sont écrits et lus depuis ces deux fils : ils sont protégés par un `Mutex`, tenu le temps d'une copie.

Entre une source et le fil de rendu ne passent que la dernière demande et la dernière image finie, chacune derrière un `Mutex` tenu le temps d'un remplacement.

L'instantané est le seul état du cœur partagé entre fils. Il se publie par remplacement atomique d'un `Arc` (`arc-swap` ou un `Mutex<Arc<Snapshot>>` tenu le temps d'un clonage).

## Décisions techniques

**D-01 Bindings libobs maison.** Le crate `obs-wrapper` n'est plus maintenu (dernière version 0.4.1, septembre 2022). Les bindings sont générés par bindgen à partir des en-têtes d'OBS 32, présents dans OBS Flatpak (`/app/include/obs`) et dans le paquet `obs-studio` natif. On limite la génération à l'API utilisée (liste blanche) et on commite le résultat, pour que le build n'exige ni libclang ni en-têtes. Un script les régénère au changement de version majeure d'OBS : `scripts/gen-libobs-bindings.sh [include-dir]`, qui pilote le générateur `tools/gen-libobs-bindings` (projet cargo exclu du workspace, seul à dépendre de bindgen). La liste blanche nomme chaque fonction utilisée, garde opaques les types qu'`obs_source_info` référence sans que le plugin s'en serve, et génère les enums en constantes, pour qu'une valeur inconnue venue d'OBS ne soit jamais un comportement indéfini. La disposition des structures est vérifiée à la compilation.

**D-02 D-Bus avec zbus 5.** Pur Rust, maintenu, sans dépendance à libdbus. On n'utilise pas le crate `mpris`, qui passe par libdbus et n'apporte rien pour deux interfaces en lecture seule. Chaque appel de méthode expire après une seconde, et une erreur d'appel vaut donnée manquante : un lecteur figé ne bloque jamais les autres. La journalisation passe par la façade `log`, que la colle OBS relaiera vers `blog`.

**D-03 Images par le crate `image`**, limité aux formats rencontrés chez les lecteurs réels : JPEG, PNG, et WebP (AudioTube). Un seul chemin pour `data:`, `file://` et les pochettes en ligne : des octets, décodés en RGBA hors du fil graphique. Le fil de rendu recadre ensuite l'image sur son carré central, la met à l'échelle de l'emplacement en moyennant en lumière linéaire les pixels que couvre chaque pixel produit, et arrondit ses coins, directement dans l'image de la source : la miniature n'a pas de texture à elle. Cette version mise à l'échelle est gardée par source et refaite seulement quand l'image, le côté de l'emplacement ou le rayon change, pour qu'une nouvelle seconde ne remette pas la pochette à l'échelle. Un chargement qui dépasse deux secondes (fichier sur un montage réseau figé) est abandonné et compte comme un échec, pour que l'état des autres lecteurs continue d'avancer. Écarté : `gs_image_file`, qui n'accepte qu'un chemin de fichier et forcerait à écrire les miniatures `data:` sur disque.

**D-04 Rendu du texte : rastérisation en Rust par `cosmic-text` 0.17**, tranchée par l'essai de la phase 3 (2026-10-14). La source entière, texte compris, est dessinée en une image RGBA par le crate `render`, puis montrée comme une texture. Options comparées :

| Option | Pour | Contre |
|--------|------|--------|
| Sources texte privées d'OBS (`text_ft2_source_v2`) en sources enfants | aucun rastériseur à maintenir, polices système d'OBS | contrôle limité sur la troncature et l'alignement, dépend du comportement d'une autre source |
| Rastérisation en Rust (`cosmic-text`, mise en forme et repli de polices) vers une texture | contrôle total, même rendu en natif et en Flatpak | dépendance lourde, découverte des polices à gérer |

Critères de choix : repli correct sur les caractères japonais (cas réel de mpv), troncature dans une largeur donnée, alpha du texte, coût CPU. L'option `cosmic-text` les remplit tous, l'autre n'a pas été essayée. Constats :

- **Japonais.** « 愛をとりもどせ!! (MOVIE ver.) » s'affiche sur l'hôte et dans le bac à sable d'OBS Flatpak 32.2.2, avec des images identiques au pixel près : le bac à sable voit les polices de l'hôte sous `/run/host/fonts`, Noto Sans CJK et Noto Color Emoji compris. Le runtime seul n'a aucune police CJK, c'est donc l'hôte qui doit en fournir une. Les idéogrammes suivent la langue du système (Noto Sans CJK SC hors d'une locale japonaise), les kanas Noto Sans CJK JP.
- **Police par défaut.** C'est celle de `cosmic-text` (Open Sans, puis Noto Sans, DejaVu Sans, FreeSans), et non celle de fontconfig : `fontdb` retient le dernier alias `sans-serif` qu'il lit, Nimbus Sans Narrow sur l'hôte et FreeSans dans le bac à sable, là où `fc-match` donne Noto Sans et DejaVu Sans. Suivre exactement fontconfig demanderait de lier libfontconfig ou de refaire son ordre de préférence.
- **Troncature.** Coupure sur les avances des glyphes mis en forme, vérifiée par une seconde mise en forme, points de suspension compris dans la largeur.
- **Alpha.** La couverture des glyphes se mélange en lumière linéaire dans l'image prémultipliée, comme le reste de la source.
- **Coût.** La lecture des polices du système prend 80 ms à chaud et 2,1 s à froid (2 868 faces sur le poste de développement). Un redessin du panneau complet avec miniature (600 × 156) prend 0,55 ms, mais le premier d'une piste monte à 3,6 ms (pochette 360 × 360) et 12 ms (1024 × 1024, le maximum gardé par le cœur), et le premier usage d'une police de repli coûte quelques millisecondes. Le dessin tourne donc sur un fil dédié, qui charge aussi les polices, et le fil graphique ne fait que demander des images et envoyer la dernière reçue en texture.
- **glibc.** Le rastériseur de swash (`zeno`) appelle `acosf`, qu'un hôte en glibc 2.43 ou plus lie en version 2.43 (`acosf@GLIBC_2.43`), absente du runtime 25.08 (2.42). `crates/obs-mpris/build.rs` passe `-Wl,--wrap=acosf` à l'édition de liens de la cdylib, qui définit `__wrap_acosf` à partir du crate `libm` : la bibliothèque n'exige pas plus que GLIBC_2.39, comme avant `cosmic-text`. `crates/render/build.rs` fait de même pour les exemples, qui tournent ainsi dans le bac à sable.

**D-05 Position extrapolée** (B-17). Recalage sur `Seeked`, sur changement de piste ou d'état, et relecture périodique de `Position` à un intervalle à régler à l'usage, pour les lecteurs qui omettent `Seeked`.

**D-06 Choix mémorisé et clé de lecteur.** Le réglage mémorise le nom de bus de l'instance choisie (B-03). La clé de lecteur (B-04) en est dérivée à la demande, jamais stockée en double. Ordre « premier » : alphabétique sur la clé, puis sur le nom de bus, sans tenir compte de la casse (B-05).

**D-07 Frontière FFI étanche.** Chaque rappel `extern "C"` passe par `catch_unwind`. Les erreurs sont journalisées via `blog`, jamais propagées à OBS.

**D-08 Client HTTP `ureq`**, bloquant, léger, TLS par rustls, utilisé seulement dans le fil réseau avec un délai d'expiration court. Pas de runtime asynchrone ajouté pour quelques requêtes. Précisé à l'implémentation (phase 5) :

- `ureq` 3 avec ses fonctions par défaut : rustls sur `ring`, et les certificats racines de `webpki-roots` compilés dans le plugin. Le plugin ne dépend donc ni du magasin de certificats de l'hôte ni de celui du runtime : vérifié dans le bac à sable d'OBS Flatpak le 2026-10-14 (MusicBrainz, Cover Art Archive avec sa redirection vers archive.org, `i.ytimg.com`).
- Délais : 3 s pour la connexion, 5 s pour une requête entière, redirections et corps compris. Une erreur réseau ou un délai dépassé est un échec sans nouvelle tentative. Les statuts 429, 500, 502, 503 et 504 ont droit à une seule nouvelle tentative, après 2 s ou après le `Retry-After` demandé s'il ne dépasse pas 10 s.
- Corps lus jusqu'à la même limite que les miniatures locales (32 Mio), seule une réponse 200 non vide compte.
- User-Agent `obs-dbus-mpris/<version du crate> ( <adresse du dépôt> )`, l'adresse venant de la constante `REPOSITORY` de `crates/mpris-core/src/network.rs`, qui vaut `https://gitlab.com/iglou.eu/obs-dbus-mpris`.
- glibc : la bibliothèque n'exige toujours pas plus que GLIBC_2.39, `ring` n'ajoute aucun symbole plus récent que celui du runtime 25.08 (2.42).

**D-09 Services de pochettes.** Tranché le 2026-10-14 par une étude sur 13 pistes réelles et 8 témoins (`~/Sourcery/Etudes/obs-dbus-mpris-pochettes-en-ligne/rapport.md`, qui donne la séquence de requêtes, le nettoyage, le score minimal et ses tests). MusicBrainz et le Cover Art Archive : 7 pochettes justes, 0 fausse une fois retirée la variante « X by Y » du nettoyage, qui causait la seule erreur. User-Agent `obs-dbus-mpris/<version> ( <URL du dépôt> )`, une requête par seconde au plus, une nouvelle tentative sur un refus de débit, image `front-500`. Wikipédia, avec les images non libres de l'article de l'album. YouTube, miniature déduite de l'identifiant de la vidéo. Précisé à l'implémentation (phase 5) : la variante « X by Y » est retirée du nettoyage, sans autre variante qu'`Artiste - Titre` pour une piste sans artiste, et un test garde la BO de *Cowboy Bebop* sans pochette. Pour une recherche d'album, Wikipédia passe l'article d'un album avant celui d'un single du même nom, comme MusicBrainz, ce qui donne la pochette de l'album *Back in Black* là où l'étude trouvait le rond de label du single. La miniature YouTube (`hqdefault`, 480 × 360) est recadrée sur le carré central de son image 16:9, sans les bandes noires. Écartés : ListenBrainz (mêmes données, accès stable sous jeton), Wikidata (2 images sur 21), TheAudioDB (abonnement payant pour publier), Discogs (images non libres, société commerciale), et les services commerciaux comme Deezer, plus complets (9 justes) mais hors du cadre voulu.

| Service | Particularités |
|---------|----------------|
| API Deezer | une requête de recherche, pochette fournie dans la réponse |
| iTunes Search API | une requête de recherche, pochette redimensionnable par l'URL |
| MusicBrainz et Cover Art Archive | données ouvertes, deux requêtes, une requête par seconde au plus, User-Agent identifiable exigé |

Critères de choix : couverture sur des pistes réelles (japonaises comprises), justesse du résultat, conditions d'utilisation compatibles avec un plugin distribué.

**D-10 Défilement par bandes** (B-36). Un texte trop long pour sa ligne, titre, sous-titre ou message de repli, n'est pas dessiné dans l'image de la source. Le fil de rendu le dessine une fois dans une bande à part : le texte, un écart de deux fois sa taille de police, puis assez de sa propre copie pour couvrir la ligne. La période de la bande, largeur du texte arrondie au pixel supérieur plus l'écart, est un nombre entier de pixels, si bien que la copie tombe sur la même fraction de pixel que le texte et que la fenêtre décalée d'une période montre exactement le début. Chaque bande devient une texture, que `video_render` dessine à chaque image par `gs_draw_sprite_subregion`, une fenêtre de la taille de la ligne, à un décalage en pixels entiers : rien ne sort de la ligne, et le texte reste net. Le décalage vient de `Scroll::offset`, calcul pur en nanosecondes entières, sans dérive au fil des heures : le cycle commence par la pause, puis tous les textes partent ensemble à la même vitesse jusqu'à ce que le plus long ait parcouru sa période, le plus court attendant à sa période, qui se dessine comme son début. Le cycle repart quand les bandes changent (texte, taille, ligne, couleur, pictogramme pause qui raccourcit la ligne du titre) ou quand la vitesse ou la pause change. À une vitesse de 0 (B-36), ou pour une bande qui dépasserait 8 192 pixels de large, le texte est coupé par des points de suspension dans l'image, comme avant. La vitesse est la seule manière d'arrêter le défilement : aucune case ne double ce réglage.

## Builds

**Natif.** `cargo build --release` produit la cdylib, installée sous le nom `obs-dbus-mpris.so` :

```
~/.config/obs-studio/plugins/obs-dbus-mpris/
  bin/64bit/obs-dbus-mpris.so
  data/locale/fr-FR.ini, en-US.ini
```

Le plugin ne lie pas `libobs` à l'édition de liens : ses symboles se résolvent au chargement, dans le processus d'OBS qui a déjà chargé `libobs`. Le build n'a ainsi besoin ni de `libobs` ni de ses en-têtes, seulement des bindings commités (D-01). rustc ne passe pas `-z defs` et active `BIND_NOW` : tout se résout au `dlopen`, qu'OBS fait en `RTLD_NOW`. La cdylib `libobs_mpris.so` est installée sous le nom `obs-dbus-mpris.so`.

Les propriétés sont rangées dans des groupes titrés d'OBS (`obs_properties_add_group`, `OBS_GROUP_NORMAL`), dans l'ordre de B-51, chaque réglage dans le groupe que lui donne le schéma. OBS garde les valeurs des propriétés groupées parmi les réglages de la source elle-même, au même niveau que les autres (constaté dans `obs-properties.c` et dans la vue des propriétés d'OBS 32, qui construit les enfants d'un groupe avec les réglages de la source) : les réglages restent plats, et leur lecture ne dépend pas des groupes. Un groupe porte un nom (`group_player`, etc.) distinct de toute clé de réglage, puisqu'OBS refuse une propriété dont le nom est déjà pris.

La clé de traduction d'une propriété est sa clé de réglage, celle du titre d'un groupe son nom, et un test vérifie que chaque fichier de langue contient exactement les clés utilisées. Le schéma dit quels réglages portent un texte d'aide (B-52), et la clé de ce texte se dérive de la clé du réglage : `<clé>.Help`, comme `scroll_speed.Help`. Le point la tient à l'écart de toute clé de réglage. La glue le passe à `obs_property_set_long_description`, qu'OBS montre en infobulle. Chaque phrase y tient sa ligne (`\n` dans le fichier de langue, qu'OBS convertit), puisqu'une infobulle en texte brut ne revient jamais à la ligne d'elle-même.

**Contrôle avant chargement.** `scripts/lib/plugin.sh` réunit ce que partagent les scripts qui construisent le plugin : le build release, la disposition du module (`bin/64bit`, `data/locale`) et le contrôle de la bibliothèque contre sa cible. Ce contrôle refuse une bibliothèque qui exige une glibc plus récente que celle de la cible, qui appelle un symbole absent de la `libobs` de la cible, ou à laquelle manque un point d'entrée qu'OBS exige (`obs_module_load`, `obs_module_ver`, `obs_module_set_pointer`). Sans `libobs` sur la machine de build, comme en CI pour l'archive native, seul le contrôle des symboles `libobs` est sauté : il est fait sur la même bibliothèque par le build Flatpak.

**Développement avec OBS Flatpak.** `scripts/install-dev.sh [--native]` construit le plugin en release et le copie (par défaut dans le dossier Flatpak, avec `--native` dans `~/.config/obs-studio/plugins`). Le même `.so` natif est copié dans `~/.var/app/com.obsproject.Studio/config/obs-studio/plugins/obs-dbus-mpris/` (mêmes sous-dossiers `bin/64bit` et `data/locale`), qu'OBS Flatpak charge directement. Il tourne alors avec la glibc du runtime (2.42), plus ancienne que celle de l'hôte : le script d'installation vérifie qu'aucun symbole plus récent n'est requis, et que chaque symbole `libobs` attendu existe dans la `libobs` d'OBS Flatpak. La seule référence trop récente, `acosf@GLIBC_2.43` venue du rastériseur de glyphes, est enveloppée à l'édition de liens (D-04).

**Version et journal des changements.** La version se fixe en un seul endroit, la table `[workspace.package]` du `Cargo.toml` racine, dont les quatre crates héritent. Elle nomme les fichiers de la release et entre dans le User-Agent (D-08). Les changements de chaque version sont dans `CHANGELOG.md`, au format Keep a Changelog, et une release se marque par un tag `v<version>`.

**Archive native.** `scripts/package-native.sh [dossier]` produit `obs-dbus-mpris-<version>-x86_64-linux.tar.gz`, dans `target/dist` par défaut. Elle se décompresse dans `~/.config/obs-studio/plugins/` et y dépose le dossier `obs-dbus-mpris/` : `bin/64bit/obs-dbus-mpris.so`, `data/locale/*.ini` et `LICENSE`. Une bibliothèque se lie à la glibc de son système de build : la release se construit donc en CI sur Debian 12 (image `rust:1.87-bookworm`, glibc 2.36), et se charge sur Debian 12, Ubuntu 24.04 et toute distribution plus récente (construite ainsi le 2026-10-14, elle n'exige pas plus que GLIBC_2.35). Le script refuse une bibliothèque qui exige plus que `GLIBC_MAX`, 2.36 par défaut. Construite sur le poste de développement (glibc 2.43), elle exige 2.39, à cause des références faibles de la bibliothèque standard de Rust à `pidfd_spawnp` et `pidfd_getpid` : une archive locale demande alors `GLIBC_MAX=2.39`, et ne vaut que pour des systèmes aussi récents.

**Intégration continue.** `.gitlab-ci.yml`, trois étapes :

| Étape | Job | Image | Ce qu'il fait |
|-------|-----|-------|---------------|
| test | `test` | `rust:1.87-bookworm` | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, avec `dbus-daemon` pour les tests d'intégration et DejaVu Sans pour les tests de texte |
| build | `native` | `rust:1.87-bookworm` | `scripts/package-native.sh`, l'archive en artefact |
| build | `flatpak` | `ghcr.io/flathub-infra/flatpak-github-actions:freedesktop-25.08` | installe OBS Flatpak stable et l'extension `rust-stable`, puis `scripts/build-flatpak.sh`, le bundle en artefact |
| release | `upload` | `alpine:3.22` | sur un tag `v*` seulement : dépose l'archive et le bundle dans le registre de paquets générique du projet, et tire du journal des changements la section de la version |
| release | `release` | `registry.gitlab.com/gitlab-org/cli:latest` | sur un tag `v*` seulement : crée la release GitLab par le mot-clé `release`, avec cette section pour description et des liens vers les deux fichiers du registre |

Les fichiers de la release vivent dans le registre de paquets générique, et non en artefacts de job, qui expirent. Le job `upload` échoue si le tag ne correspond pas à la version du workspace, puisque les fichiers construits portent cette dernière. Le job `flatpak` suppose un conteneur privilégié, que bubblewrap demande pour isoler chaque étape du build : c'est le cas des runners hébergés de GitLab.com (`saas-linux-*`), pas forcément d'un runner auto-hébergé.

**Flatpak.** Le manifeste `packaging/flatpak/com.obsproject.Studio.Plugin.DbusMpris.yml` construit une extension de l'application OBS. Disposition relevée sur l'extension Tuna installée et dans les métadonnées d'OBS (`directory=plugins`, `merge-dirs=lib/obs-plugins;share/obs/obs-plugins`) :

```
runtime          com.obsproject.Studio, branche stable
sdk              org.freedesktop.Sdk//25.08
sdk-extensions   org.freedesktop.Sdk.Extension.rust-stable
build-extension  true
préfixe          /app/plugins/DbusMpris
fichiers         lib/obs-plugins/obs-dbus-mpris.so
                 share/obs/obs-plugins/obs-dbus-mpris/locale/*.ini
                 share/licenses/com.obsproject.Studio.Plugin.DbusMpris/obs-dbus-mpris/LICENSE
                 share/metainfo/com.obsproject.Studio.Plugin.DbusMpris.metainfo.xml
```

La licence est copiée d'office par flatpak-builder, qui installe le `LICENSE` de la racine des sources d'un module. Le fichier AppStream décrit l'extension comme un module complémentaire (`type="addon"`, `<extends>com.obsproject.Studio</extends>`), comme les extensions d'OBS publiées sur Flathub, pour que les logithèques la rattachent à OBS.

Le build se fait hors ligne. `packaging/flatpak/cargo-sources.json` décrit chaque crate de `Cargo.lock` comme une archive de crates.io, vérifiée par la somme de contrôle du lockfile, et ajoute une configuration Cargo qui remplace crates.io par ces crates déposées dans les sources. flatpak-builder les télécharge avant le build, qui tourne ensuite sans réseau (`cargo --offline --locked`). Ce fichier se régénère à chaque changement de `Cargo.lock`, avec `flatpak-cargo-generator.py` du dépôt flatpak-builder-tools (dossier `cargo`), qui demande les modules Python `aiohttp` et `tomlkit`, dans un environnement virtuel au besoin :

```sh
python3 -m venv /tmp/fcg && /tmp/fcg/bin/pip install aiohttp tomlkit
/tmp/fcg/bin/python flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
```

Le générateur ne contacte le réseau que pour des dépendances tirées d'un dépôt git, que le workspace n'a pas. `ring` compile du C et de l'assembleur avec le gcc du SDK, `webpki-roots` n'est qu'un crate de données : ni l'un ni l'autre ne demande rien de plus au SDK.

L'extension `rust-stable` fournit le Rust stable du moment (1.98.1 au 2026-10-14), et non la 1.87 de `rust-toolchain.toml`, que seul rustup lit. Le workspace, en édition 2024 et `rust-version` 1.87, se construit avec toute version plus récente, et le build sur l'hôte garde la 1.87.

`scripts/build-flatpak.sh [dossier]` (`target/flatpak` par défaut) lance flatpak-builder, contrôle la bibliothèque construite contre la glibc du runtime (2.42) et la `libobs` d'OBS Flatpak, puis produit le bundle `obs-dbus-mpris-<version>-x86_64.flatpak` (`flatpak build-bundle --runtime`). Il n'installe rien : OBS Flatpak, `org.freedesktop.Sdk//25.08` et son extension `rust-stable//25.08` doivent être là. Le bundle s'installe par `flatpak install --user <bundle>`. Il n'est pas signé : Flatpak lui crée un dépôt d'origine sans vérification GPG, comme pour Tuna, et une nouvelle version s'installe par un nouveau bundle. L'extension et une copie dans le dossier de plugins utilisateur d'OBS Flatpak (`install-dev.sh`) ne coexistent pas : OBS charge l'extension en premier et ignore la copie, avec pour seule trace une ligne « Skipping legacy plugin » dans son journal (observé sur OBS 33.0.0 le 2026-10-25). `install-dev.sh` refuse donc d'installer tant que l'extension est présente.

Vérifié le 2026-10-14, dans une installation Flatpak à part : le bundle s'installe (dépôt d'origine `dbusmpris-origin`), le bac à sable d'OBS 32.2.2 montre la bibliothèque dans `/app/plugins/lib/obs-plugins` et les traductions dans `/app/plugins/share/obs/obs-plugins/obs-dbus-mpris/locale`, et la bibliothèque s'y charge en `RTLD_NOW` après la `libobs` d'OBS, tous ses symboles résolus. Le build de l'extension n'exige pas plus que GLIBC_2.39 (les mêmes références faibles à `pidfd_*`), sous les 2.42 du runtime.

La permission D-Bus passe par un override (B-45). L'accès réseau nécessaire à B-18 est déjà accordé à OBS Flatpak (`shared=network`).

## Tests

- **Unitaires, test d'abord, dans `mpris-core`** : règle de sélection (scénario 13 pas à pas, lecteur mémorisé présent mais arrêté, règle réévaluée à chaque image pendant le délai de grâce, lecteur en pause affiché faute de lecteur en lecture, autre lecteur arrêté jamais retenu ni suivi, lecteur habituel lancé avant puis après la fin du délai, limite exacte des 5 minutes, aucun autre lecteur pris après le délai, lecteur suivi disparu puis relancé, premier lecteur apparu retenu quand rien n'a encore été affiché, choix manuel définitif, « Aucun choix » choisi à la main, Annuler pendant et après le délai, après plusieurs modifications et après le bouton par défaut, retour à l'ancien lecteur une fois les propriétés rouvertes, indépendance à l'ordre des lecteurs), extraction de la clé, extrapolation de la position, lecture des métadonnées (manquantes, mal typées), décodage des miniatures (`data:`, `file://`, format non pris en charge, données corrompues), format des temps, temps restant (nul en fin de piste et au-delà, compté sur les secondes affichées), conversion des réglages et valeurs par défaut, groupe de chaque réglage et ordre du schéma, pochette en ligne (nettoyage et score minimal portés du contrat de l'étude, construction des recherches, lecture des réponses des services à partir de réponses réelles enregistrées par l'étude dans `crates/mpris-core/tests/fixtures/`, réduites aux champs lus, réponses mal formées, identifiant YouTube, séquence des requêtes sur un `fetch` simulé, cache et dédoublonnage, échecs compris, rythme et nouvelles tentatives). Aucun test n'appelle le vrai service par défaut. Un test ignoré, `cargo test -p mpris-core --test online_live -- --ignored --nocapture`, rejoue les pistes de l'étude contre les vrais services, et l'exemple `cargo run --example look_up -- <service> <titre> <artiste> [album]` fait une seule recherche, y compris dans le bac à sable d'OBS Flatpak.
- **Unitaires dans `render`**, sans OBS : disposition (largeur totale, hauteur tirée des polices, de l'épaisseur de la barre et des éléments cochés, ou fixée avec les tailles mises à l'échelle dans leurs proportions, marges proportionnelles, flip, bornes du schéma, aucun rectangle hors du cadre), dessin (couleurs et alpha en lumière linéaire, coins arrondis, barre au pixel près, pictogramme pause, chaque texte dans sa rangée), miniature (recadrage central, mise à l'échelle, cache), troncature par points de suspension, clé de redessin (deux clés égales donnent la même image, une progression de moins d'un pixel garde la clé), fil de rendu (seule la dernière demande est dessinée, l'arrêt n'attend pas que les sources disparaissent), défilement (pause d'abord, vitesse constante, texte court qui attend le long, pause commune, boucle sans dérive sur des durées énormes, vitesse et pause nulles), bandes (période, fenêtre à la période identique au début, écart vide, couleur, repli sur les points de suspension, bandes gardées d'une seconde à l'autre). Les tests de texte chargent un seul fichier de police connu, DejaVu Sans ou Noto Sans, ou celui que nomme `RENDER_TEST_FONT`, et se sautent sans lui.
- **Contrôle visuel du rendu** : l'exemple `cargo run --release -p render --example render_png -- <dossier>` dessine en PNG chaque variante de la maquette de B-37, titre japonais et titre trop long compris, ainsi que l'écart, l'image ronde, le temps restant et des hauteurs fixes, et mesure le temps de dessin. Pour une variante dont des textes défilent, il écrit aussi l'image seule, chaque bande, et la source composée à quelques instants du cycle, comme OBS la dessine. Le même exécutable se lance dans le bac à sable d'OBS Flatpak (`flatpak run --command=<chemin absolu de l'exemple> com.obsproject.Studio <dossier>`), pour voir le rendu avec les polices que voit OBS.
- **Intégration D-Bus** : un faux lecteur MPRIS servi par zbus sur un bus privé (`dbus-run-session -- cargo test`). Scénarios : apparition, disparition, changement de métadonnées, `Seeked`, relance avec un nouveau suffixe d'instance. Pour la pochette en ligne, le cœur démarre avec une recherche simulée (`Core::start_with`), sans réseau : une seule recherche par piste et par service, rien sans vœu (B-48), retour à une piste sans nouvelle requête (scénario 9), vœu retiré, pochette illisible, arrêt rapide pendant une recherche bloquée. Un binaire de test à part compte les fils : pas de fil réseau avant la première recherche, et plus après l'arrêt du cœur. L'exemple `watch --online <service>` montre la recherche sur les lecteurs réels, et `watch --settled` ou `watch --manual` la règle de sélection après le délai de grâce ou après un choix manuel, sans attendre 5 minutes.
- **Colle OBS** : scénarios d'acceptation manuels ([besoins.md](besoins.md), section 7) dans OBS Flatpak.

## Risques

| Risque | Traitement |
|--------|------------|
| Build Rust hors ligne en Flatpak | résolu en phase 7 : crates fournies par `cargo-sources.json`, build de l'extension vérifié le 2026-10-14 |
| Rendu du texte et repli CJK | résolu par l'essai de la phase 3 (D-04) : le bac à sable voit les polices de l'hôte sous `/run/host/fonts`, mais le runtime seul n'a aucune police CJK. La documentation d'installation demandera donc une police CJK sur l'hôte |
| Miniatures hors du bac à sable d'OBS Flatpak (Chrome en Flatpak) | option de pochette en ligne (B-18) |
| Pochette en ligne fausse ou introuvable (titres de vidéos non normalisés) | accepté : l'option est sur « Aucune » par défaut, le score minimal écarte les candidats douteux, un échec laisse l'image par défaut |
| Croissance de la mémoire sur une longue session avec pochette en ligne | le cache garde les pochettes telles que téléchargées (50 à 100 Ko), décodées seulement pour les pistes affichées |
| Build natif jamais chargé dans un OBS natif | accepté pour l'instant, validation reportée |
| Écarts d'ABI de libobs entre versions mineures | bindings générés depuis OBS 32, testés sur 32.x uniquement |
| Lecteurs qui omettent `Seeked` ou renvoient une position fausse | relecture périodique (D-05) |
