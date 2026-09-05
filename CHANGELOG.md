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

- **Convention changelog du parc posée sur ce dépôt** : ce fichier, les hooks `pre-commit` et
  `pre-push` dans `.githooks/`, et le workflow `changelog-guard.yml` qui rejoue les mêmes
  contrôles en CI au moment du tag. Ce dépôt en était dépourvu alors qu'il est déployé, ce qui
  le laissait hors de la garantie que les autres ont.

