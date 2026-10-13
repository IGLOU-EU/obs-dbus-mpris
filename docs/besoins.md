# Besoins

Chaque exigence porte un identifiant B-xx, repris par la feuille de route et par les tests.

## 1. Lecteurs et sélection

**B-01 Découverte.** La source détecte en continu les lecteurs présents sur le bus de session (noms `org.mpris.MediaPlayer2.*`), apparitions et disparitions comprises, sans redémarrer OBS.

**B-02 Sélecteur.** Les propriétés de la source proposent une entrée par instance de lecteur présente sur le bus, libellée par le titre de ce qu'elle joue (`xesam:title`), à défaut par le nom lisible du lecteur (`Identity`). Plusieurs instances d'un même lecteur, plusieurs mpv par exemple, se distinguent ainsi. La liste est triée selon l'ordre défini en B-05.

**B-03 Mémorisation.** L'instance choisie dans le sélecteur est mémorisée dans les réglages de la source par son nom de bus complet (`org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA`), donc propre à chaque source et sauvegardée avec la collection de scènes. Si elle est absente à l'ouverture des propriétés, elle reste dans la liste, libellée par la clé de son lecteur (B-04) et marquée « (indisponible) », pour que le choix ne se perde pas.

**B-04 Clé de lecteur.** La clé d'un lecteur est le premier segment qui suit `org.mpris.MediaPlayer2.` dans son nom de bus. La spécification MPRIS réserve les segments suivants aux instances multiples, et ils changent à chaque lancement : `org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA` a pour clé `mpv`. La clé se déduit du nom de bus mémorisé, elle n'est jamais stockée, et sert à retrouver le lecteur quand l'instance mémorisée a disparu (B-06).

**B-05 Règle de sélection.** Le lecteur effectif est réévalué en continu, dans cet ordre :

1. l'instance mémorisée, si elle est présente, quel que soit son état,
2. sinon une instance du même lecteur (même clé), celle qui est en lecture de préférence, sinon la première,
3. sinon le premier lecteur en lecture (`PlaybackStatus` = `Playing`),
4. sinon le premier lecteur disponible,
5. sinon aucun, et la source affiche le message de repli (B-23).

« Premier » s'entend dans l'ordre alphabétique des clés (B-04), puis des noms de bus, sans tenir compte de la casse, pour un résultat stable tant que les lecteurs tournent.

La case « Sélection automatique », cochée par défaut, commande les étapes 2 à 4. Décochée, seule l'étape 1 s'applique : sans l'instance mémorisée exacte, même après une relance du même lecteur, la source affiche le message de repli jusqu'à un nouveau choix. Une source dédiée à un lecteur ne bascule ainsi jamais sur un autre.

Conséquence voulue de l'étape 1 : si l'instance mémorisée est présente mais arrêtée pendant qu'un autre lecteur joue, la source affiche le message de repli. Le choix explicite prime sur l'activité des autres lecteurs.

**B-06 Retour automatique.** Le repli ne modifie jamais le choix mémorisé. Quand l'instance mémorisée a disparu et qu'un lecteur de même clé apparaît (relance de mpv avec un nouveau suffixe d'instance, par exemple), la source le suit d'elle-même (étape 2), si la sélection automatique est cochée.

**B-07 Nouvelle source.** Une source sans choix mémorisé applique la règle B-05 à partir de l'étape 3, ou affiche le message de repli si la sélection automatique est décochée.

## 2. Contenu affiché

Données lues : dans `Metadata`, les clés `xesam:title`, `xesam:artist`, `xesam:albumArtist`, `xesam:album`, `mpris:artUrl`, `mpris:length`, ainsi que les propriétés `PlaybackStatus`, `Position`, `Rate` et `Identity`.

Chaque élément s'affiche ou se masque par une case à cocher indépendante.

**B-10 Miniature.** Image tirée de `mpris:artUrl`. Formats pris en charge : `data:` encodé en base64 (cas réel de mpv, qui fournit `data:image/jpeg;base64,…`) et `file://`. Les URL `http(s)://` sont hors du but actuel, aucun lecteur utilisé ne les impose. Une miniature absente, illisible, dans un format non pris en charge ou qui met plus de deux secondes à charger laisse l'emplacement vide, sans erreur visible.

**B-11 Titre.** `xesam:title`.

**B-12 Artiste.** `xesam:artist` (une liste, jointe par « , »), à défaut `xesam:albumArtist`.

**B-13 Album.** `xesam:album`.

Artiste et album forment le **sous-titre**. Leur disposition (une ligne ou deux, séparateur) relève de l'organisation visuelle, traitée plus tard.

**B-14 Barre de progression.** Proportion position sur durée.

**B-15 Position.** Le temps écoulé dans la piste.

**B-16 Durée.** La durée totale de la piste.

Format des temps : `m:ss`, et `h:mm:ss` à partir d'une heure. Quand la durée est inconnue (flux, `mpris:length` absent ou nul), la durée et la barre ne s'affichent pas, la position reste affichable.

**B-17 Exactitude de la position.** MPRIS ne signale pas les changements de `Position`. La position affichée est donc extrapolée à partir de la dernière valeur connue, du temps écoulé et de `Rate` (1.0 par défaut), et recalée sur le signal `Seeked`, sur chaque changement de piste ou d'état, et par une relecture périodique. Affichage à la seconde, écart toléré d'une seconde.

**B-18 Pochette en ligne, en option.** En Flatpak, OBS ne peut généralement pas lire les fichiers hors de son bac à sable, et notamment pas les fichiers où d'autres applications Flatpak déposent leur pochette. Constaté le 2026-10-13 : Chrome la met dans son `/tmp` privé, AudioTube dans son dossier `~/.var/app`, tous deux invisibles depuis OBS Flatpak. Une case dans les propriétés, décochée par défaut, autorise alors la source à chercher la pochette dans une base en ligne :

- la recherche n'a lieu que si la miniature locale (B-10) est absente ou illisible,
- elle se fait par artiste et album, à défaut par artiste et titre,
- une seule requête par piste distincte, résultat mis en cache pour la session, échecs compris, pour ne jamais interroger le service en boucle,
- la requête ne bloque rien et expire vite. Un échec laisse l'emplacement vide, comme en B-10.

Le service se choisit par un essai sur des pistes réelles, japonaises comprises, parmi des services sans clé d'API (voir D-09).

## 3. États de lecture

**B-20 Lecture.** Affichage complet selon les cases cochées.

**B-21 Pause.** Affichage complet, temps et barre figés, et un indicateur de pause. La forme de l'indicateur relève de l'organisation visuelle.

**B-22 Arrêt ou absence.** Lecteur effectif arrêté (`Stopped`), aucun lecteur, ou bus de session injoignable : la source affiche le message de repli seul.

**B-23 Message de repli.** Champ texte dans les propriétés. Valeur par défaut traduite : « Aucune lecture » en français, « Nothing playing » en anglais. Un champ vide rend la source entièrement transparente dans ces états.

## 4. Apparence

**B-30 Flip.** Une case « Miniature à droite » place la miniature à droite. Par défaut, elle est à gauche.

**B-31 Couleurs.** Quatre couleurs avec canal alpha, pour permettre la transparence : fond, texte, remplissage de la barre, piste de la barre (la partie non remplie).

**B-32 Une couleur de texte** commune au titre, au sous-titre et aux temps.

**B-33 Taille de la source.** Largeur et hauteur en pixels, puisqu'OBS n'offre ni positionnement relatif ni mise en page adaptative. La miniature est carrée, de côté égal à la hauteur.

**B-34 Tailles de police.** Trois tailles en pixels : titre, sous-titre, temps.

**B-35 Police.** Police sans empattement par défaut du système, sans choix de police dans les propriétés. Le rendu doit se replier sur une police capable d'afficher les écritures non latines : le cas est réel, mpv expose aujourd'hui un album au titre japonais.

**B-36 Débordement.** Aucun élément ne déborde du cadre de la source, quelle que soit la longueur des textes. Le traitement des textes trop longs (points de suspension ou défilement) relève de l'organisation visuelle.

## 5. Propriétés de la source

Récapitulatif des réglages d'une source. Les valeurs par défaut des couleurs et des tailles sont des propositions à ajuster à l'usage.

| Clé | Type | Défaut proposé | Besoin |
|-----|------|----------------|--------|
| `player` | liste (nom de bus de l'instance) | vide (aucun choix mémorisé) | B-02 à B-07 |
| `auto_select` | case | oui | B-05 à B-07 |
| `show_art` | case | oui | B-10 |
| `fetch_art_online` | case | non | B-18 |
| `show_title` | case | oui | B-11 |
| `show_artist` | case | oui | B-12 |
| `show_album` | case | oui | B-13 |
| `show_progress` | case | oui | B-14 |
| `show_position` | case | oui | B-15 |
| `show_duration` | case | oui | B-16 |
| `art_right` | case | non | B-30 |
| `color_background` | couleur RGBA | `#000000`, alpha 60 % | B-31 |
| `color_text` | couleur RGBA | `#FFFFFF`, alpha 100 % | B-31, B-32 |
| `color_bar_fill` | couleur RGBA | `#FFFFFF`, alpha 100 % | B-31 |
| `color_bar_track` | couleur RGBA | `#FFFFFF`, alpha 25 % | B-31 |
| `width` | entier (px) | 600 | B-33 |
| `height` | entier (px) | 120 | B-33 |
| `font_size_title` | entier (px) | 32 | B-34 |
| `font_size_subtitle` | entier (px) | 24 | B-34 |
| `font_size_time` | entier (px) | 20 | B-34 |
| `fallback_text` | texte | traduit | B-23 |

## 6. Exigences non fonctionnelles

**B-40 Aucune interface hors de la source.** Ni entrée dans Outils, ni dock, ni réglage global.

**B-41 Rendu jamais bloqué.** Aucun appel D-Bus, aucune lecture de fichier, aucun décodage d'image dans les rappels de rendu d'OBS.

**B-42 Robustesse.** Un lecteur qui disparaît, un bus absent, des métadonnées manquantes ou mal formées, une miniature illisible ne font jamais planter OBS et ne figent jamais le rendu. Aucune panique Rust ne traverse la frontière FFI.

**B-43 Sobriété.** Rien n'est régénéré quand rien ne change. Le texte des temps n'est redessiné qu'au changement de seconde, les autres textes et la miniature qu'au changement de piste ou de réglage.

**B-44 Deux builds.** Un `.so` natif x86_64 et l'extension Flatpak `com.obsproject.Studio.Plugin.DbusMpris` pour la branche stable d'OBS (runtime `org.freedesktop.Platform` 25.08).

**B-45 Permission Flatpak.** Une extension Flatpak ne peut pas ajouter de permission à l'application hôte, et les métadonnées d'OBS Flatpak 32.2.2 ne déclarent pas l'accès à `org.mpris.MediaPlayer2.*` (constaté le 2026-10-13, l'accès actuel vient d'un override utilisateur). La documentation d'installation donne la commande :

```sh
flatpak override --user --talk-name='org.mpris.MediaPlayer2.*' com.obsproject.Studio
```

**B-46 Langues.** Interface en français (fr-FR) et en anglais (en-US).

**B-47 Licence.** GPL-2.0-or-later.

**B-48 Aucun accès réseau par défaut.** Le plugin n'émet aucune requête réseau tant qu'aucune source n'a coché l'option B-18.

## 7. Critères d'acceptation

La version 1.0 est acceptée quand ces scénarios passent dans OBS Flatpak. Le build natif est produit, mais sa validation dans un OBS natif est reportée.

1. mpv joue un fichier avec pochette intégrée. Une source ajoutée affiche miniature, titre, artiste, album, barre, position et durée, et la position avance chaque seconde.
2. Mise en pause de mpv : indicateur de pause, temps figé. Reprise : le temps repart sans saut.
3. Avance rapide dans mpv : la position affichée suit en moins d'une seconde.
4. Fermeture de mpv : message de repli. Relance de mpv (nouveau suffixe d'instance) : la source revient sur mpv sans intervention.
5. Lecteur mémorisé absent, deux autres lecteurs présents dont un en lecture : la source affiche celui qui joue.
6. Chaque case masque son élément. Le flip déplace la miniature. Des couleurs à alpha nul rendent les zones concernées transparentes. Largeur, hauteur et tailles de police sont respectées.
7. Un titre très long, ou en japonais, s'affiche lisiblement et ne déborde pas du cadre.
8. Deux sources réglées sur deux lecteurs différents vivent côte à côte sans interférer.
10. Trois instances de mpv jouent : le sélecteur propose trois entrées, chacune libellée par son titre, et la source affiche exactement l'instance choisie.
11. Sélection automatique décochée, instance mémorisée fermée alors que d'autres lecteurs jouent : message de repli. Relance du même lecteur : toujours le message de repli, jusqu'à un nouveau choix dans le sélecteur.
9. Une piste dont la miniature locale est illisible : option B-18 décochée, emplacement vide et aucune requête réseau. Option cochée, une pochette trouvée en ligne s'affiche, et changer de piste puis revenir ne relance pas de requête.

## 8. Questions ouvertes

À trancher au moment où elles deviennent utiles, pas avant.

- **Organisation visuelle** : placement et alignement des éléments, marges, disposition du sous-titre, forme de l'indicateur de pause, rendu sans miniature, miniatures non carrées (VLC fournit une vignette YouTube en 16:9 alors que l'emplacement est carré), traitement des textes trop longs. Phase dédiée dans la feuille de route.
- **Service de pochettes en ligne** : choisi par essai au moment d'implémenter B-18.
- **Forge de publication** (GitLab ou GitHub) : reportée à la phase de distribution.
- **Validation dans un OBS natif** : reportée. Aucun OBS natif n'est installé sur le poste de développement, le build natif est produit sans y être testé.
