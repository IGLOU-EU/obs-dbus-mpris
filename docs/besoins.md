# Besoins

Chaque exigence porte un identifiant B-xx, repris par la feuille de route et par les tests.

## 1. Lecteurs et sélection

**B-01 Découverte.** La source détecte en continu les lecteurs présents sur le bus de session (noms `org.mpris.MediaPlayer2.*`), apparitions et disparitions comprises, sans redémarrer OBS.

**B-02 Sélecteur.** Les propriétés de la source proposent une entrée par instance de lecteur présente sur le bus, libellée par le titre de ce qu'elle joue (`xesam:title`), à défaut par le nom lisible du lecteur (`Identity`). Plusieurs instances d'un même lecteur, plusieurs mpv par exemple, se distinguent ainsi. Une première entrée « Aucun choix » efface le choix mémorisé (B-07). La liste est triée selon l'ordre défini en B-05.

**B-03 Mémorisation.** L'instance choisie dans le sélecteur est mémorisée dans les réglages de la source par son nom de bus complet (`org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA`), donc propre à chaque source et sauvegardée avec la collection de scènes. Si elle est absente à l'ouverture des propriétés, elle reste dans la liste, libellée par la clé de son lecteur (B-04) et marquée « (indisponible) », pour que le choix ne se perde pas.

**B-04 Clé de lecteur.** La clé d'un lecteur est le premier segment qui suit `org.mpris.MediaPlayer2.` dans son nom de bus. La spécification MPRIS réserve les segments suivants aux instances multiples, et ils changent à chaque lancement : `org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA` a pour clé `mpv`. La clé se déduit du nom de bus mémorisé, elle n'est jamais stockée, et sert à retrouver le lecteur quand l'instance mémorisée a disparu (B-06).

**B-05 Règle de sélection.** La sélection automatique sert au démarrage : elle laisse le temps de lancer sa source habituelle, puis se fige pour qu'aucun changement automatique ne survienne en plein direct. Chaque source retient en mémoire le lecteur qu'elle affiche, son lecteur suivi (B-08).

**Délai de grâce.** Pendant les 5 minutes qui suivent le démarrage de la source (lancement d'OBS ou ajout de la source), et tant qu'aucun choix n'a été fait à la main dans les propriétés, le lecteur affiché est réévalué en continu, dans cet ordre :

1. l'instance mémorisée, si elle est présente, quel que soit son état,
2. sinon une instance du lecteur mémorisé (même clé), celle qui est en lecture de préférence, sinon la première,
3. sinon un autre lecteur disponible, le premier en lecture (`PlaybackStatus` = `Playing`) de préférence, sinon le premier en pause. Un lecteur arrêté n'est pas retenu ici : il n'afficherait que le message de repli, et ne doit pas devenir le lecteur suivi,
4. sinon aucun, et la source affiche le message de repli (B-23).

Lancer sa source habituelle pendant le délai la fait donc afficher, à la place d'un autre lecteur affiché faute de mieux.

**Après le délai.** La source n'en change plus d'elle-même. Elle garde le lecteur suivi, ou une instance de même clé s'il est relancé (mpv quitté puis relancé avec un nouveau suffixe d'instance), même si l'instance mémorisée réapparaît : en plein direct, ce pourrait être une erreur de manipulation, alors que le lecteur affiché était le bon. Si le lecteur suivi disparaît sans relance, elle affiche le message de repli en attendant son retour, et ne bascule jamais sur un autre lecteur. Seule exception : si la source n'a encore rien affiché depuis son démarrage, le premier lecteur qui apparaît est retenu selon les étapes 1 à 3, puis suivi.

**Choix manuel.** Un lecteur choisi à la main dans les propriétés est définitif : il ne rouvre pas de délai de grâce, il devient aussitôt le lecteur suivi, et la source ne le quitte plus d'elle-même. S'il disparaît, elle affiche le message de repli jusqu'à son retour ou sa relance. Choisir « Aucun choix » à la main laisse la source sans lecteur suivi, hors délai : le premier lecteur qui apparaît est retenu puis suivi.

« Premier » s'entend dans l'ordre alphabétique des clés (B-04), puis des noms de bus, sans tenir compte de la casse.

La case « Sélection automatique », cochée par défaut, commande tout ce qui précède. Décochée, seule l'instance mémorisée exacte s'affiche, quel que soit son état : sans elle, même après une relance du même lecteur, la source affiche le message de repli jusqu'à un nouveau choix. Cocher ou décocher la case compte comme un choix manuel : pas de délai de grâce.

**Annuler.** OBS applique chaque modification des propriétés en direct, puis rétablit l'ancien réglage si l'on annule. Un réglage du lecteur qui revient à sa valeur d'avant la modification rétablit l'état de sélection d'avant (lecteur suivi, délai de grâce en cours ou non) : annuler n'est pas un choix manuel.

**B-06 Retour automatique.** Le repli ne modifie jamais le choix mémorisé dans les réglages : seul un choix manuel le change. Pendant le délai de grâce, quand l'instance mémorisée réapparaît ou qu'un lecteur de même clé apparaît, la source y revient d'elle-même (étapes 1 et 2). Après le délai, ou après un choix manuel, seule la relance du lecteur suivi le ramène.

**B-07 Nouvelle source.** Une source sans choix mémorisé applique la règle B-05 à partir de l'étape 3, ou affiche le message de repli si la sélection automatique est décochée.

**B-08 Lecteur suivi et délai de grâce.** Le lecteur suivi, l'instant de démarrage de la source et le fait qu'un choix manuel a eu lieu sont retenus en mémoire seulement : ils ne sont pas enregistrés dans les réglages et repartent de zéro au redémarrage d'OBS. Le choix mémorisé dans les réglages redevient alors la source habituelle, avec un nouveau délai de grâce. Le délai de 5 minutes est fixe.

## 2. Contenu affiché

Données lues : dans `Metadata`, les clés `xesam:title`, `xesam:artist`, `xesam:albumArtist`, `xesam:album`, `xesam:url`, `mpris:artUrl`, `mpris:length`, ainsi que les propriétés `PlaybackStatus`, `Position`, `Rate` et `Identity`.

Chaque élément s'affiche ou se masque par une case à cocher indépendante.

**B-10 Miniature.** Image tirée de `mpris:artUrl`. Formats pris en charge : `data:` encodé en base64 (cas réel de mpv, qui fournit `data:image/jpeg;base64,…`) et `file://`. Les URL `http(s)://` sont hors du but actuel, aucun lecteur utilisé ne les impose. Une miniature absente, illisible, dans un format non pris en charge ou qui met plus de deux secondes à charger laisse à sa place une image par défaut, sans erreur visible : l'emoji partition de musique (🎼, U+1F3BC), d'une seule couleur, celle du texte, sur la couleur du fond. Si aucune police installée ne contient ce symbole, l'emplacement reste uni.

**B-11 Titre.** `xesam:title`.

**B-12 Artiste.** `xesam:artist` (une liste, jointe par « , »), à défaut `xesam:albumArtist`.

**B-13 Album.** `xesam:album`.

Artiste et album forment le **sous-titre**, sur une ligne, dans la forme que fixe B-37.

**B-14 Barre de progression.** Proportion position sur durée.

**B-15 Position.** Le temps écoulé dans la piste.

**B-16 Durée.** La durée totale de la piste.

**B-19 Temps restant.** Une case remplace, à droite sous la barre, la durée par le temps restant, écrit `-m:ss` (durée moins position). Elle n'a d'effet que si la durée s'affiche, donc si elle est cochée et connue.

Format des temps : `m:ss`, et `h:mm:ss` à partir d'une heure. Quand la durée est inconnue (flux, `mpris:length` absent ou nul), la durée et la barre ne s'affichent pas, la position reste affichable.

**B-17 Exactitude de la position.** MPRIS ne signale pas les changements de `Position`. La position affichée est donc extrapolée à partir de la dernière valeur connue, du temps écoulé et de `Rate` (1.0 par défaut), et recalée sur le signal `Seeked`, sur chaque changement de piste ou d'état, et par une relecture périodique. Affichage à la seconde, écart toléré d'une seconde.

**B-18 Pochette en ligne, en option.** En Flatpak, OBS ne peut généralement pas lire les fichiers hors de son bac à sable, et notamment pas les fichiers où d'autres applications Flatpak déposent leur pochette. Constaté le 2026-10-13 : Chrome la met dans son `/tmp` privé, AudioTube dans son dossier `~/.var/app`, tous deux invisibles depuis OBS Flatpak. Un choix « Pochette en ligne » dans les propriétés autorise alors la source à chercher la pochette en ligne, auprès d'un seul service à la fois :

- **Aucune** (par défaut) : aucune recherche.
- **MusicBrainz** : la base musicale libre, images du Cover Art Archive. Recherche par artiste et album, à défaut par artiste et titre, après nettoyage des métadonnées (suffixes comme « (Remastered) », titres « Artiste - Titre »). Un candidat n'est retenu que s'il passe un score minimal de ressemblance, pour qu'une piste sans correspondance n'ait pas de pochette plutôt qu'une fausse.
- **Wikipédia** (en anglais) : l'image de l'article de l'album ou du morceau, pour les disques connus seulement.
- **YouTube** : la miniature de la vidéo, déduite de son identifiant quand le lecteur donne l'adresse d'une page YouTube (`xesam:url`), sans recherche. Image servie par Google. Constaté le 2026-10-14 : LibreWolf donne cette adresse, Chrome (Chromium) non.

Pour tous :

- la recherche n'a lieu que si la miniature locale (B-10) est absente ou illisible,
- une seule recherche par piste distincte, résultat mis en cache pour la session, échecs compris, pour ne jamais interroger le service en boucle,
- la requête ne bloque rien et expire vite. Un échec laisse l'image par défaut, comme en B-10.

Le choix des services vient d'une étude sur les pistes réelles (D-09, `~/Sourcery/Etudes/obs-dbus-mpris-pochettes-en-ligne/`). Une seule base libre de pochettes tient la route, MusicBrainz. Wikipédia et YouTube complètent la liste à la demande d'Adrien, en connaissant leurs limites.

## 3. États de lecture

**B-20 Lecture.** Affichage complet selon les cases cochées.

**B-21 Pause.** Affichage complet, temps et barre figés, et un indicateur de pause. L'indicateur est un pictogramme pause (deux barres verticales) à la couleur du texte, au bout de la ligne de titre. Titre masqué, il n'y a pas d'indicateur de pause.

**B-22 Arrêt ou absence.** Lecteur effectif arrêté (`Stopped`), aucun lecteur, ou bus de session injoignable : la source affiche le message de repli seul.

**B-23 Message de repli.** Champ texte dans les propriétés. Valeur par défaut traduite : « Aucune lecture » en français, « Nothing playing » en anglais. Un champ vide rend la source entièrement transparente dans ces états.

## 4. Apparence

**B-30 Flip.** Une case « Miniature à droite » place la miniature à droite. Par défaut, elle est à gauche.

**B-31 Couleurs.** Quatre couleurs avec canal alpha, pour permettre la transparence : fond, texte, remplissage de la barre, piste de la barre (la partie non remplie).

**B-32 Une couleur de texte** commune au titre, au sous-titre et aux temps.

**B-33 Taille de la source.** La largeur se règle en pixels, puisqu'OBS n'offre ni positionnement relatif ni mise en page adaptative. La hauteur se calcule d'après les tailles de police et les éléments cochés, jamais d'après la piste en cours : la source garde ainsi la même taille quoi qu'il joue, et ne fait pas sauter la scène. La miniature est carrée, de côté égal à cette hauteur, et compte dans la largeur.

**B-37 Disposition.** Quatre blocs visuels : titre, sous-titre, progression (barre puis temps), image. Le panneau de texte et l'image sont deux blocs distincts, séparés par l'écart de B-39, chacun avec ses coins arrondis, sans fond derrière l'image. Une miniature non carrée est recadrée au centre. Le sous-titre s'écrit « Album - Artiste », ou l'un des deux seul s'il manque l'autre. Sous la barre, la position est alignée à gauche et la durée à droite. Les marges et les écarts sont proportionnels aux tailles de police, sans réglage dédié. Maquette de référence, des éléments les plus réduits à la disposition complète, miniature à gauche puis à droite :

```
╭───────────────────────────────────────────────────╮
│                                                   │
│  TITRE                                            │
│                                                   │
╰───────────────────────────────────────────────────╯
╭───────────────────────────────────────────────────╮
│                                                   │
│  TITRE                                            │
│  Nom Album - Artist                               │
│                                                   │
╰───────────────────────────────────────────────────╯
╭───────────────────────────────────────────────────╮
│                                                   │
│  TITRE                                            │
│  Nom Album - Artist                               │
│                                                   │
│  ════════════════════════════════───────────────  │
│  1:21                                       1:56  │
│                                                   │
╰───────────────────────────────────────────────────╯
╭──────────────────╮╭───────────────────────────────────────────────────╮
│                  ││                                                   │
│                  ││  Titre                                            │
│                  ││  Nom Album - Artist                               │
│       IMAGE      ││                                                   │
│                  ││  ════════════════════════════════───────────────  │
│                  ││  1:21                                       1:56  │
│                  ││                                                   │
╰──────────────────╯╰───────────────────────────────────────────────────╯
╭───────────────────────────────────────────────────╮╭──────────────────╮
│                                                   ││                  │
│  Titre                                            ││                  │
│  Nom Album - Artist                               ││                  │
│                                                   ││       IMAGE      │
│  ════════════════════════════════───────────────  ││                  │
│  1:21                                       1:56  ││                  │
│                                                   ││                  │
╰───────────────────────────────────────────────────╯╰──────────────────╯
```

**B-38 Coins arrondis.** Un rayon en pixels, appliqué au panneau et à l'image. 0 donne des coins carrés. Une case « Image ronde » dessine l'image en disque, quel que soit ce rayon, qui reste celui du panneau. L'emplacement de l'image par défaut prend la même forme.

**B-50 Épaisseur de la barre.** L'épaisseur de la barre de progression se règle en pixels. La hauteur de la source la suit (B-33).

**B-39 Écart image-panneau.** Un écart en pixels entre l'image et le panneau de texte, 0 pour les coller. Il compte dans la largeur totale (B-33).

**B-34 Tailles de police.** Trois tailles en pixels : titre, sous-titre, temps.

**B-35 Police.** Police sans empattement par défaut de `cosmic-text` : Open Sans, à défaut Noto Sans, puis DejaVu Sans, puis FreeSans, selon ce qui est installé (D-04). Aucun choix de police dans les propriétés. Un caractère absent de cette police est pris dans une police installée qui l'a, ce qui couvre les écritures non latines : le cas est réel, mpv expose aujourd'hui un album au titre japonais.

**B-36 Débordement.** Aucun élément ne déborde du cadre de la source, quelle que soit la longueur des textes. Un texte trop long (titre, sous-titre, message de repli) défile de droite à gauche dans sa ligne, toujours dans le même sens et sans saut : il est suivi d'un écart fixe de deux fois sa taille de police, puis de sa propre copie, si bien que la boucle reprend sans discontinuité. Chaque fois que le texte revient à sa position de départ, il marque une pause, dont la durée se règle, puis repart. Il commence aussi par cette pause. La vitesse se règle en pixels par seconde. Quand le titre et le sous-titre défilent tous deux, ils sont synchronisés : ils partent ensemble à la même vitesse, celui qui revient au début le premier attend l'autre, puis la pause s'applique aux deux. Un texte qui tient dans sa ligne ne bouge pas. Le cycle repart du début à chaque changement de texte, de largeur de ligne (apparition du pictogramme pause) ou de réglage du défilement. Le défilement continue pendant la pause du lecteur. Une vitesse de 0 désactive le défilement : le texte trop long est alors tronqué par des points de suspension. Un texte d'aide le dit à côté du réglage (B-52).

## 5. Propriétés de la source

**B-51 Groupes.** Les propriétés sont rangées en groupes titrés, dans cet ordre : Lecteur (`player`, `auto_select`), Éléments affichés (les cases `show_*`, puis la liste « Pochette en ligne »), Disposition (largeur, image, coins, barre), Texte (tailles de police, défilement, message de repli), Couleurs. L'ordre du tableau ci-dessous suit celui des groupes.

**B-52 Textes d'aide.** Deux réglages portent un texte d'aide, affiché par OBS en infobulle : la vitesse de défilement (ce qu'elle règle, et que 0 désactive le défilement), et la pochette en ligne (quand la recherche a lieu, ce que fait chaque service, ce qui lui est envoyé, et qu'Aucune n'envoie rien).

Récapitulatif des réglages d'une source. Les valeurs par défaut des couleurs et des tailles sont des propositions à ajuster à l'usage.

| Groupe | Clé | Type | Défaut proposé | Besoin |
|--------|-----|------|----------------|--------|
| Lecteur | `player` | liste (nom de bus de l'instance) | vide (aucun choix mémorisé) | B-02 à B-07 |
| Lecteur | `auto_select` | case | oui | B-05 à B-07 |
| Éléments affichés | `show_art` | case | oui | B-10 |
| Éléments affichés | `show_title` | case | oui | B-11 |
| Éléments affichés | `show_artist` | case | oui | B-12 |
| Éléments affichés | `show_album` | case | oui | B-13 |
| Éléments affichés | `show_progress` | case | oui | B-14 |
| Éléments affichés | `show_position` | case | oui | B-15 |
| Éléments affichés | `show_duration` | case | oui | B-16 |
| Éléments affichés | `show_remaining` | case | non | B-19 |
| Éléments affichés | `art_online` | liste (Aucune, MusicBrainz, Wikipédia, YouTube) | Aucune | B-18, B-52 |
| Disposition | `width` | entier (px) | 600 | B-33 |
| Disposition | `art_right` | case | non | B-30 |
| Disposition | `art_round` | case | non | B-38 |
| Disposition | `art_gap` | entier (px) | 0 | B-39 |
| Disposition | `corner_radius` | entier (px) | 12 | B-38 |
| Disposition | `bar_height` | entier (px) | 6 | B-50 |
| Texte | `font_size_title` | entier (px) | 32 | B-34 |
| Texte | `font_size_subtitle` | entier (px) | 24 | B-34 |
| Texte | `font_size_time` | entier (px) | 20 | B-34 |
| Texte | `scroll_speed` | entier (px/s, 0 = désactivé) | 40 | B-36, B-52 |
| Texte | `scroll_pause` | entier (ms) | 2000 | B-36 |
| Texte | `fallback_text` | texte | traduit | B-23 |
| Couleurs | `color_background` | couleur RGBA | `#000000`, alpha 60 % | B-31 |
| Couleurs | `color_text` | couleur RGBA | `#FFFFFF`, alpha 100 % | B-31, B-32 |
| Couleurs | `color_bar_fill` | couleur RGBA | `#FFFFFF`, alpha 100 % | B-31 |
| Couleurs | `color_bar_track` | couleur RGBA | `#FFFFFF`, alpha 25 % | B-31 |

## 6. Exigences non fonctionnelles

**B-40 Aucune interface hors de la source.** Ni entrée dans Outils, ni dock, ni réglage global.

**B-41 Rendu jamais bloqué.** Aucun appel D-Bus, aucune lecture de fichier, aucun décodage d'image dans les rappels de rendu d'OBS.

**B-42 Robustesse.** Un lecteur qui disparaît, un bus absent, des métadonnées manquantes ou mal formées, une miniature illisible ne font jamais planter OBS et ne figent jamais le rendu. Aucune panique Rust ne traverse la frontière FFI.

**B-43 Sobriété.** Rien n'est régénéré quand rien ne change. L'image de la source n'est redessinée que quand ce qu'elle montre change : une nouvelle seconde, un pixel de plus sur la barre, une autre piste, un autre état de lecture ou un autre réglage. La miniature n'est remise à l'échelle qu'au changement d'image, de taille ou de rayon des coins.

**B-44 Deux builds.** Un `.so` natif x86_64 et l'extension Flatpak `com.obsproject.Studio.Plugin.DbusMpris` pour la branche stable d'OBS (runtime `org.freedesktop.Platform` 25.08).

**B-45 Permission Flatpak.** Une extension Flatpak ne peut pas ajouter de permission à l'application hôte, et les métadonnées d'OBS Flatpak 32.2.2 ne déclarent pas l'accès à `org.mpris.MediaPlayer2.*` (constaté le 2026-10-13, l'accès actuel vient d'un override utilisateur). La documentation d'installation donne la commande :

```sh
flatpak override --user --talk-name='org.mpris.MediaPlayer2.*' com.obsproject.Studio
```

**B-46 Langues.** Interface en français (fr-FR) et en anglais (en-US).

**B-47 Licence.** GPL-2.0-or-later.

**B-48 Aucun accès réseau par défaut.** Le plugin n'émet aucune requête réseau tant qu'aucune source n'a choisi un service de pochette en ligne (B-18). Le fil réseau ne démarre qu'à la première recherche.

## 7. Critères d'acceptation

La version 1.0 est acceptée quand ces scénarios passent dans OBS Flatpak. Le build natif est produit, mais sa validation dans un OBS natif est reportée.

1. mpv joue un fichier avec pochette intégrée. Une source ajoutée affiche miniature, titre, artiste, album, barre, position et durée, et la position avance chaque seconde.
2. Mise en pause de mpv : indicateur de pause, temps figé. Reprise : le temps repart sans saut.
3. Avance rapide dans mpv : la position affichée suit en moins d'une seconde.
4. Fermeture de mpv : message de repli. Relance de mpv (nouveau suffixe d'instance) : la source revient sur mpv sans intervention.
5. À l'ajout de la source, lecteur mémorisé absent, deux autres lecteurs présents dont un en lecture : la source affiche celui qui joue.
6. Chaque case masque son élément. Le flip déplace la miniature. Des couleurs à alpha nul rendent les zones concernées transparentes. La largeur totale, miniature comprise, et les tailles de police sont respectées. La hauteur suit les tailles de police et les éléments cochés (B-33).
7. Un titre très long, ou en japonais, s'affiche lisiblement et ne déborde pas du cadre.
8. Deux sources réglées sur deux lecteurs différents vivent côte à côte sans interférer.
10. Trois instances de mpv jouent : le sélecteur propose trois entrées, chacune libellée par son titre, et la source affiche exactement l'instance choisie.
11. Sélection automatique décochée, instance mémorisée fermée alors que d'autres lecteurs jouent : message de repli. Relance du même lecteur : toujours le message de repli, jusqu'à un nouveau choix dans le sélecteur.
12. Titre et sous-titre trop longs : pause, départ ensemble à la vitesse réglée, le plus court attend au début, pause commune, sans saut. Vitesse de défilement à 0 : points de suspension. Écart image-panneau, image ronde avec et sans pochette, temps restant qui décompte au rythme de la position.
13. mpv mémorisé, OBS lancé alors que seul Chrome joue : Chrome s'affiche. mpv lancé dans les 5 minutes : la source passe sur mpv. Variante : mpv lancé après 5 minutes, la source reste sur Chrome. Hors délai, une vidéo démarrée dans un autre lecteur ne change rien. Lecteur suivi fermé puis relancé : la source le suit. Fermé pour de bon : message de repli, sans basculer sur un autre lecteur qui joue. Lecteur choisi à la main dans les propriétés pendant le délai : il s'affiche aussitôt, et un autre lecteur qui démarre ensuite, même l'habituel, ne le remplace pas.
14. Les propriétés s'affichent en cinq groupes titrés (B-51). L'épaisseur de la barre change la barre et la hauteur de la source, l'image restant carrée (B-50).
9. Une piste dont la miniature locale est illisible : pochette en ligne sur Aucune, image par défaut et aucune requête réseau. Sur MusicBrainz, une pochette trouvée s'affiche (Supertramp, Crime Of The Century), une piste sans correspondance garde l'image par défaut, et changer de piste puis revenir ne relance pas de requête. Sur YouTube, une vidéo ouverte dans LibreWolf affiche sa miniature.

## 8. Questions ouvertes

À trancher au moment où elles deviennent utiles, pas avant.

- **Organisation visuelle** : la disposition est fixée par B-37. Validée dans OBS le 2026-10-14. Les réglages visuels demandés ensuite (écart, image ronde, défilement, temps restant) sont spécifiés en B-19, B-36, B-38 et B-39.
- **Service de pochettes en ligne** : tranché le 2026-10-14 par une étude (D-09).
- **Forge de publication** : GitLab, `https://gitlab.com/iglou.eu/obs-dbus-mpris` (choisie le 2026-10-14, dépôt pas encore en ligne).
- **Validation dans un OBS natif** : reportée. Aucun OBS natif n'est installé sur le poste de développement, le build natif est produit sans y être testé.
