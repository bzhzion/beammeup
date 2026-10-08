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

- **`beammeup key` connaît beaucoup plus de touches** : les flèches, `home`, `end`, `pageup`,
  `pagedown`, `delete`, `insert`, `backspace`, `space`, `f1` à `f12`, `shift-tab`, `ctrl-<lettre>`
  (toute la série, pas seulement `ctrl-c`, `ctrl-d` et `ctrl-z`), `alt-<touche>`, et les
  combinaisons `ctrl-`/`alt-`/`shift-` sur les flèches et les touches de navigation
  (`ctrl-left`, `shift-f5`). Insensible à la casse, `+` accepté comme `-`, un caractère seul est
  envoyé tel quel (`y`), et une touche inconnue est refusée avec la liste complète. **Les flèches
  s'envoyaient déjà** par `send` avec la séquence d'échappement, vérifié par un essai réel
  (PowerShell reçoit `DownArrow`), mais ce chemin n'était documenté nulle part et dépend de la
  façon dont chaque shell écrit le caractère ESC (`$([char]27)`, `$'\x1b'`). La table est écrite
  une seule fois, dans `keys.rs`, fonction pure testée (8 tests, **prouvés rouges** par trois
  mutations : mauvaise lettre de flèche, poids du modificateur, octet de contrôle décalé).
  ⚠️ Séquences xterm en mode curseur normal : un programme passé en *mode curseur applicatif* attend
  `ESC O A` et peut ignorer les flèches, BeamMeUp ne suit pas ce mode ; `send` avec la séquence
  exacte reste le recours. Les six anciennes touches gardent exactement leurs octets (test de
  non-régression). ⚠️ **Au moment de poser le tag, retirer la mention « first release after 1.0.4 »**
  du README et du skill (`grep -rn "after 1.0.4"`), qui existe pour ne pas annoncer à un utilisateur
  de la 1.0.4 des touches qu'elle n'a pas.

- **Page de politique de confidentialité** (`site/privacy.html`), publiée à
  `beammeup.breizhzion.com/privacy.html` : aucune télémétrie, risque HTTP non chiffré de l'accès
  distant, stockage local, aucun tiers. Demandée par un modérateur winget avant validation de la
  PR de soumission `Breizhzion.BeamMeUp` 1.0.4. Reprend la navigation réelle du site
  (sidebar/pillnav/topbar/panel) plutôt qu'une mise en page ad hoc.
- **Publication winget automatique à chaque tag** : `release-windows.yml` appelle désormais
  `publish-winget.yml` dans un job `winget` distinct, après le build, sur un push de tag
  `vX.Y.Z` uniquement (jamais sur un rejeu par `workflow_dispatch`, qui ne produirait qu'un
  doublon). Le paquet existe en amont depuis la fusion de la PR #424365 le 2026-10-08.
  ⚠️ **Volontairement pas en `continue-on-error`**, contrairement à justmakeq, dont l'étape winget
  est restée en panne des semaines sans que rien ne le montre : un échec rend le run rouge, la
  release étant déjà publiée par le job précédent. `publish-winget.yml` reste lançable à la main.
  ⚠️ **La politique de confidentialité doit suivre ce que l'application écrit sur le disque** :
  une version publiée automatiquement ne passe plus sous les yeux de personne avant d'être en
  ligne.
- **`robots.txt`, `sitemap.xml` et `llms.txt` pour le site** : jusqu'ici Cloudflare Pages répondait
  **200 avec la page d'accueil en HTML** à ces trois adresses, donc un robot lisait du HTML en
  guise de `robots.txt`. Aucune date de modification dans le plan du site, qui serait devenue fausse
  au premier changement.
- **Garde-fou `scripts/verifier-ecritures-disque.py`** qui bloque la publication winget (première
  étape de `publish-winget.yml`) et fait échouer la CI (`ci.yml`, job `politique`) quand la
  politique de confidentialité a pu devenir fausse. Deux contrôles dérivés du code : chaque nom de
  fichier écrit en dur doit figurer dans `site/privacy.html`, et l'inventaire des points
  d'écriture (`scripts/ecritures-disque.json`) doit correspondre au code. Le second ne dit pas
  que la politique est fausse, il dit qu'il faut la relire ; après relecture, `--maj` enregistre
  le nouvel inventaire. **Prouvé rouge** par deux mutations (un point d'écriture ajouté, un nom
  de fichier non cité), puis restauré.
- **Essai à blanc de la publication winget** (`dry_run` de `publish-winget.yml`) : exécute tout le
  chemin réel jusqu'à la génération du manifeste, sans ouvrir de pull request. Permet d'éprouver
  le workflow sans version nouvelle à publier.

### Corrigé

- **Le site et le README affirmaient « aucun token stocké » et que le port de capture était
  « restreint au compte »** : faux dans les deux cas. Le token d'accès distant est écrit en clair
  dans `remote.json` dès `beammeup web on` (le README le disait lui-même plus bas, donc il se
  contredisait), et le port de capture d'écran reste joignable par n'importe quel processus de la
  machine, ce que la section Security du README admettait déjà et que le site contredisait. Les
  deux textes disent maintenant : aucun mot de passe ni clé SSH stockés, le seul secret gardé est le
  token d'accès distant ; le canal de commande est restreint au compte, le port de capture ne l'est
  pas. La capture d'écran est aussi indiquée comme propre à Windows sur le site, et le README dit
  que `web off` n'efface pas le token. Même défaut que la politique de confidentialité, relevé en
  lisant le site contre le code.
- **La politique de confidentialité était encore fausse sur ce que l'application écrit sur le
  disque** (deuxième relevé, par lecture de tout le code cette fois) : elle affirmait « rien
  d'autre » et qu'aucun contenu de session n'était écrit, alors que `beammeup export` écrit le
  contenu d'un terminal dans le fichier demandé, que `beammeup screenshot` écrit un PNG de la
  fenêtre (par défaut `beammeup-screenshot.png` dans le dossier temporaire, jamais nettoyé), que
  `remote read --out` copie un fichier distant, que le moteur web embarqué garde son profil
  (`%LOCALAPPDATA%\com.breizhzion.beammeup`, 47 Mo mesurés), et que deux fichiers de travail
  existent (marqueur d'élévation Windows, socket de contrôle Linux). Tout est maintenant dit.

- **La première version de `privacy.html` affirmait que le token d'accès distant restait en
  mémoire et n'était jamais écrit sur disque : faux.** `remote_web.rs` sérialise `RemoteConfig`
  (bind, token, autostart) dans `remote.json` à chaque `beammeup web on`, chemin réel vérifié dans
  le code (`dirs::config_dir()`) : `%APPDATA%\beammeup\remote.json` sur Windows — **pas**
  `%LOCALAPPDATA%`, contrairement à ce qu'affirme un commentaire du code lui-même — et
  `~/.config/beammeup/remote.json` sur Linux (également faux pour les snippets, documentés à tort
  sous `~/.local/share/beammeup`). La page documente maintenant aussi que `beammeup web off`
  n'efface pas le token du fichier, et qu'aucune commande CLI ne le fait aujourd'hui : la seule
  suppression possible est manuelle. Repéré par un modérateur winget sur la PR, pas par une
  relecture interne.

### Corrigé

- **Lien de licence du footer** (`site/index.html`) pointait vers `LICENSE`, absent du dépôt
  (le fichier s'appelle `LICENSE.md`) : 404 silencieux depuis la mise en ligne du site.

### Modifié

- **Corrections issues de l'audit GEO du site** : une section FAQ de neuf questions, visible et en
  données structurées `FAQPage` (texte identique, vérifié par script) ; `screenshot` et `sameAs` dans
  les données `SoftwareApplication` ; un fichier `_headers` qui pose une politique de sécurité du
  contenu stricte (possible parce que les pages n'ont aucun script ni style en ligne),
  `Strict-Transport-Security` et `Permissions-Policy` ; la page de confidentialité référencée par
  son adresse finale `/privacy` (Cloudflare redirigeait `privacy.html` en 308, y compris dans le
  plan du site) avec son `canonical` ; description, lien vers le site et sujets du dépôt GitHub,
  qui n'avait qu'une description en français.
- **Le site met en avant trois choses qu'il ne disait pas** : l'outil ne vole jamais le focus (SYS.12,
  comportement de la 1.0.4), l'interface côté humain, barre latérale comprise (SYS.13), et le
  durcissement de l'accès distant, dit avec ses limites (comparaison à temps constant, refus des
  requêtes de type DNS rebinding sans token, HTTP en clair à réserver à Tailscale ou à un VPN).
  Un lien « Privacy policy » en pied de page : la politique n'était accessible que depuis winget.
- **Le site montre enfin le produit, et dit à l'agent comment s'en servir** : une vraie capture de
  la fenêtre (`site/assets/real-window.png`, trois sessions et un menu à flèches dont l'agent vient
  de déplacer la sélection, centrée avec sa légende) dans la section « One command post, two operators » ; un bloc « Then
  tell your agent », texte à coller dans un `AGENTS.md` ou un `CLAUDE.md` avec les six commandes
  utiles et le lien vers la section « For AI agents » du README ; des balises de partage (Open
  Graph, Twitter, canonical, couleur de thème) et des données structurées `SoftwareApplication`,
  sans numéro de version écrit en dur. La capture est prise sur un projet de démonstration, avec une
  invite neutre : **aucun nom d'utilisateur ni chemin personnel** (contrôlé à l'œil et par recherche
  dans le fichier). Rendu vérifié dans un vrai navigateur avant mise en ligne.
- **winget est mis en avant sur le site et dans le README** (section Téléchargement, carte
  Windows, et `## Installation`). Le site affichait déjà `winget install Breizhzion.BeamMeUp`
  avant que le paquet existe en amont, donc une commande qui ne marchait pas ; elle est vraie
  depuis la fusion de la PR #424365 le 2026-10-08, vérifiée par `winget show` (1.0.4, source
  publique, `PrivacyUrl` présente).
- **`actions/checkout` et `actions/setup-node` passent en v7** dans les workflows : les versions
  posées déclaraient `using: node20`, déprécié et déjà forcé sur Node 24 par GitHub. Les quatre
  changements de rupture de ces majeures ont été lus et confrontés au parc, aucun ne s'y applique,
  et les 11 runners de l'org sont en 2.336.0 ou mieux, au-dessus du minimum 2.327.1 qu'exigent
  `checkout` v5 et `setup-node` v5. Vérifié par un build iOS réel avant propagation.


## [1.0.4] - 2026-09-18

### Corrige
- **Plus aucune commande ne ramene la fenetre au premier plan.** Chaque requete recue sur
  le canal de controle appelait `show()` **puis** `set_focus()` : la fenetre sautait
  par-dessus le travail en cours de l'humain a chaque `send`/`exec`, donc en permanence
  pendant qu'un agent travaillait. Symptome rapporte : une fenetre **reduite** ne posait
  aucun probleme (`show()` seul ne la restaure pas), une fenetre simplement **en
  arriere-plan** etait arrachee devant, sortant l'humain de ce qu'il faisait. La fenetre
  est desormais rendue **visible** sans prendre le focus : la regle « jamais de mode
  invisible » portait sur la visibilite, pas sur le premier plan.
  - ⚠️ **`select` non plus ne remonte la fenetre**, contrairement a ce que son nom laisse
    croire. Son vrai travail est de laisser **le bon onglet deja selectionne** pour quand
    l'humain revient de lui-meme. Un agent qui veut montrer quelque chose ne decide pas du
    moment ou l'humain regarde.
  - Deux gestes **humains** gardent le droit de remonter la fenetre : le clic sur l'icone
    de la zone de notification, et le relancement de l'executable a la main. Ce dernier
    passe par un champ `focus` ajoute a la requete `status` (`#[serde(default)]`, donc une
    fenetre plus ancienne sait toujours lire une requete plus recente) : un `beammeup
    status` lance par un agent le laisse a `false`.
- La **synchronisation du fork `winget-pkgs`** devient **bloquante**. En simple
  avertissement, l'etape enchainait sur un `wingetcreate update` deja condamne, qui
  echouait une minute plus tard sur un message ne nommant pas la cause : on allait
  chercher la panne du cote du paquet ou du jeton. Un echec ici a une cause probable
  unique, le fork a diverge de l'amont, et il se repare a la main. Le message d'erreur
  la nomme et donne la reparation. Chemin d'echec verifie contre l'API reelle.

## [1.0.3] - 2026-09-07

### Corrigé

- **La publication apt est appelée par le workflow de release** (`workflow_call`) et non
  plus déclenchée par un événement. Le fichier reste séparé, avec ses droits et sa clé SSH
  dédiée.
  - ⚠️ **`workflow_run` ne tire pas.** Éprouvé sur `hublot` : trois tentatives, **zéro run
    déclenché**, depuis un tag comme depuis une branche, avec des noms correspondant au
    caractère près et le fichier bien présent sur la branche par défaut. Cause non établie.
  - `workflow_call` ne dépend d'aucun événement à observer : l'appelant nomme l'appelé,
    donc soit le job est dans le run, soit il n'y est pas. Vérifiable d'un coup d'œil.
  - `needs: build` remplace l'ancien `if`, et ⚠️ `secrets: inherit` est **obligatoire** :
    un workflow appelé ne reçoit aucun secret sans lui.

### Modifié

- **`publish-apt.yml` est désormais déclenché à la suite du build** (`workflow_run`), tout en
  restant un workflow **séparé** : fichier distinct, droits distincts, et surtout une clé SSH
  dédiée qui n'a rien à voir avec les autres secrets du build.
  - ⚠️ Il était « manuel uniquement », et la conséquence était que **le dépôt apt dérivait en
    silence** : ce paquet y est resté en **0.1.0** pendant que ses releases passaient à 1.0.1
    puis 1.0.2, sans que rien ne le signale. C'est le même motif que winget — **l'unique
    étape manuelle d'une chaîne automatisée est celle qui ne se fait pas.**
  - Le déclenchement manuel ne protégeait d'ailleurs pas de grand-chose : pousser un tag et
    lancer un workflow demandent le **même** droit d'écriture, donc il évitait les
    lancements accidentels et pas les malveillants. La vraie protection reste la clé
    restreinte côté serveur par sa commande forcée, inchangée.
  - Deux garde-fous nécessaires ensemble : `conclusion == 'success'` pour qu'un build en
    échec ne publie rien, et `startsWith(head_branch, 'v')` pour qu'un push de branche ne
    déclenche rien. L'un sans l'autre laisse passer un cas.
  - ⚠️ Piège de `workflow_run` : il ne se déclenche que si le fichier est sur la branche par
    défaut, et son contexte est celui de cette branche **et non du tag**. La version se lit
    donc dans `head_branch` de l'événement, pas dans `github.ref`.

### Modifié

- Le manifeste `latest.json` déclare désormais **`linux_apt`**, qui renvoie vers
  `apt.breizhzion.com`. Le dépôt apt fait autorité et versionne lui-même dans son pool, son
  index `Packages` épinglant déjà les SHA256 : une copie versionnée sur R2 serait une
  seconde source de vérité pour le même fait.

### Corrigé

- ⚠️ **L'étape de publication R2 échouait alors que ses envois réussissaient.** À la
  première release le manifeste n'existe pas encore, donc `aws s3 cp` pour le lire échoue,
  ce qui est normal et traité. Mais **le wrapper pwsh de GitHub Actions termine par
  `exit $LASTEXITCODE`**, et `Set-Content` ne remet pas cette variable à zéro : le 1 d'`aws`
  survivait jusqu'à la fin du script.
  - Le symptôme trompait complètement : les deux envois d'installateur apparaissaient en
    succès dans le journal, juste avant un `exit code 1`, donc l'échec se lisait comme un
    problème d'envoi alors qu'il venait d'une variable rémanente.
  - La version bash du même motif, sur `noisecrypt`, n'a pas ce défaut : son `||` remet le
    code de retour à zéro de lui-même. **Le même code traduit d'un shell à l'autre n'a pas
    le même comportement d'erreur**, et c'est le genre d'écart qu'on ne voit qu'à
    l'exécution.

## [1.0.2] - 2026-09-07

### Ajouté
- **Publication sur `dl.breizhzion.com`** : l'installateur Windows part désormais aussi sur
  le bucket R2 partagé `breizhzion-releases`, sous le préfixe de l'appli, en **nom fixe et
  en copie versionnée immuable**, accompagné d'un `latest.json`.
  - Les trois formes d'URL ne servent pas à la même chose : le nom fixe pour les boutons de
    site et les `curl`, la copie versionnée pour les gestionnaires de paquets qui épinglent
    un SHA256 par version, le `latest.json` pour qu'un site affiche la version courante sans
    redéploiement. Confondre les deux premières est ce qui casse un manifest winget déjà
    accepté, et c'est arrivé pour de vrai (`microsoft/winget-pkgs#399072`).
  - ⚠️ Via l'**API S3** et non `wrangler r2 object`, et ce n'est pas une préférence : les
    permissions R2 **par bucket** ne valent que pour l'API S3, l'API Cloudflare exigeant une
    permission à l'échelle du compte. Ce dépôt étant **public**, un jeton capable d'écrire
    dans les photos de production ou les sauvegardes Portainer n'y a pas sa place. Le jeton
    employé est restreint au seul bucket des releases, et ce refus a été **prouvé** avant
    qu'il soit distribué.
  - Le lire-modifier-écrire de `latest.json` est protégé par le `concurrency` posé juste
    avant, sans lequel une version plus ancienne finissant en dernier écraserait la nouvelle.


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

