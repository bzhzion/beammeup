# Changelog

Toutes les évolutions notables de `beammeup` sont documentées ici.

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), versionnage
[SemVer](https://semver.org/lang/fr/). La section `[Unreleased]` accumule au fil de l'eau et
est renommée en numéro de version au moment de poser le tag.

Ce fichier est créé le 2026-09-05, après la mise en service : les évolutions antérieures ne sont
pas reconstituées, ce qui serait de la réécriture d'historique plutôt que de la documentation.
L'historique git reste la source de vérité pour ce qui précède.

## [Unreleased]

### Ajouté
- **`concurrency` posé sur les workflows de release**, `cancel-in-progress: false`.
  - **Préventif, et le commentaire le dit** : aujourd'hui chaque exécution publie sur le tag
    de sa propre version, donc une ancienne qui finirait en dernier n'écrase rien. Le
    garde-fou est posé **avant** la chose qu'il protège, à savoir l'alignement en cours qui
    va ajouter un manifeste et un fichier au nom générique partagés entre versions.
  - Un premier jet de ce commentaire décrivait la panne comme déjà possible ici. C'était
    faux, et corrigé avant commit : un commentaire qui décrit une défaillance inexistante
    finit par se lire comme un état de fait.
  - Le défaut a bien frappé ailleurs : sur `justmakeq` le 2026-09-06, une version partie
    34 minutes avant la suivante a fini 14 minutes après elle et l'a écrasée.


### Corrigé

- **`release-windows.yml` aurait créé un tag git nommé `main`** au premier lancement
  manuel. Il passait `tag_name: ${{ github.ref_name }}` à `action-gh-release`, ce qui vaut
  le tag sur un push de tag mais vaut **`main`** sur un `workflow_dispatch` depuis la
  branche par défaut, et l'action crée alors ce tag pour y attacher la release.
  - **Bug latent et non dormant** : il n'avait jamais tiré ici uniquement parce que ce
    workflow n'a jamais été lancé à la main. Le même code a déjà frappé sur `hublot`, qui
    porte un tag `main` à côté de `v0.1.0`, avec sa release courante posée dessus et donc
    une URL d'installateur `/download/main/...` que winget refuse.
  - Le piège est double : il ne tire **que** sur le chemin manuel, qui existe précisément
    pour rejouer une release. Il frappe donc au moment où on répare déjà autre chose, et il
    passe pour une conséquence de la panne en cours.
  - **La bonne réponse était déjà dans ce dépôt** : `release-linux.yml` reconstruit le tag
    depuis la version. Aligné dessus plutôt que d'introduire une troisième façon de nommer
    un tag. Deux workflows du même dépôt qui ne s'accordent pas là-dessus, c'est le signe
    qu'un des deux a été écrit sans regarder l'autre.

- **Convention de fins de ligne du parc posée dans `.gitattributes`.** Le bloc `run:` d'un
  workflow GitHub Actions est un script shell exécuté sur un runner Linux : un antislash de
  continuation suivi d'un retour chariot **ne continue pas** la ligne, la commande est coupée en
  deux, et le message d'erreur ne parle jamais de fins de ligne.
- Cas réel du 2026-09-07 sur `bzhzion/cabanon` : un `.yml` recommité en CRLF depuis une machine
  Windows (où `core.autocrlf` est actif) a fait échouer le déploiement de l'API sur un
  `usage: ssh`, la destination de la commande ayant disparu avec la continuation.
- LF forcé sur ce qu'exécute Linux (`*.sh`, `*.yml`, `*.yaml`, `Dockerfile`), CRLF sur ce
  qu'exécute Windows (`*.ps1`, `*.bat`, `*.cmd`), et `* text=auto` comme filet général.
  Référence : `admin/.claude/gitattributes-parc`.


### Modifié

- Le fichier de licence s'appelle désormais `LICENSE.md`, aligné sur les autres dépôts
  publics du parc et sur le `BZ-1.1.md` canonique dont il est la copie. Le texte est du
  Markdown, donc un nom sans extension le faisait servir par GitHub en texte préformaté,
  avec les `#` et les `**` visibles. Le nom ne change rien à la détection de licence :
  GitHub annonce « Other » dans les deux cas, BZ-1.1 n'étant pas répertoriée par SPDX.


### Ajouté

- **Convention changelog du parc posée sur ce dépôt** : ce fichier, les hooks `pre-commit` et
  `pre-push` dans `.githooks/`, et le workflow `changelog-guard.yml` qui rejoue les mêmes
  contrôles en CI au moment du tag. Ce dépôt en était dépourvu alors qu'il est déployé, ce qui
  le laissait hors de la garantie que les autres ont.

