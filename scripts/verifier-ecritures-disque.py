#!/usr/bin/env python3
"""Garde-fou : la politique de confidentialite doit decrire ce que l'application ecrit sur le disque.

Deux controles, tous deux DERIVES du code et non declares a la main :

1. Chaque nom de fichier ecrit en dur dans le code Rust (`remote.json`, `beammeup-screenshot.png`,
   ...) doit apparaitre dans `site/privacy.html`.
2. L'inventaire des points d'ecriture (`fs::write`, `File::create`, `temp_dir()`, `config_dir()`...)
   est compare a une reference versionnee, `scripts/ecritures-disque.json`. Un ecart ne dit pas que
   la politique est fausse, il dit qu'il faut la RELIRE : c'est un fil de declenchement, pas une
   preuve. Apres relecture, `--maj` enregistre le nouvel inventaire.

Pourquoi il existe : la premiere version de la politique affirmait que le jeton d'acces distant ne
quittait pas la memoire, alors que le code l'ecrit dans `remote.json`. Un moderateur winget l'a
releve sur la PR de soumission. Depuis que la publication winget part toute seule a chaque tag, plus
personne ne relit une version avant qu'elle soit en ligne.

Limite assumee : il ne voit pas ce qu'un moteur tiers ecrit de son cote (le profil du moteur web), et
un chemin construit dynamiquement echappe au controle 1. C'est pourquoi le controle 2 existe.
"""
import json
import re
import sys
from pathlib import Path

RACINE = Path(__file__).resolve().parent.parent
SOURCES = RACINE / "app" / "src-tauri" / "src"
POLITIQUE = RACINE / "site" / "privacy.html"
REFERENCE = Path(__file__).resolve().parent / "ecritures-disque.json"

MOTIFS = [
    "fs::write", "File::create", "OpenOptions", "create_dir_all", "fs::copy", "fs::rename",
    "tokio::fs", "tempfile", "temp_dir()", "config_dir()", "data_dir()", "data_local_dir()",
    "cache_dir()",
]
NOM_FICHIER = re.compile(r'"([A-Za-z0-9_{}\-\.]+\.(?:json|marker|png|sock|log|txt|toml|db|sqlite))"')


def lignes_de_code(chemin: Path):
    """Lignes du fichier hors commentaires : un commentaire qui cite un motif ne l'execute pas."""
    for ligne in chemin.read_text(encoding="utf-8").splitlines():
        if not ligne.strip().startswith("//"):
            yield ligne


def inventaire():
    resultat = {}
    for fichier in sorted(SOURCES.glob("*.rs")):
        compte = {}
        for ligne in lignes_de_code(fichier):
            for motif in MOTIFS:
                if motif in ligne:
                    compte[motif] = compte.get(motif, 0) + 1
        if compte:
            resultat[fichier.name] = compte
    return resultat


def noms_de_fichiers():
    noms = set()
    for fichier in sorted(SOURCES.glob("*.rs")):
        for ligne in lignes_de_code(fichier):
            noms.update(NOM_FICHIER.findall(ligne))
    return noms


def fragments(nom: str):
    """`beammeup-{}.sock` -> ['beammeup-', '.sock'] : chaque morceau fixe doit etre dans la politique."""
    return [f for f in re.split(r"\{[^}]*\}", nom) if f]


def main() -> int:
    # Sans cela, la console Windows rend les guillemets francais en caracteres de remplacement.
    sys.stdout.reconfigure(encoding="utf-8")
    politique = POLITIQUE.read_text(encoding="utf-8")
    erreurs = []

    for nom in sorted(noms_de_fichiers()):
        manquants = [f for f in fragments(nom) if f not in politique]
        if manquants:
            erreurs.append(
                f"le code ecrit « {nom} » mais site/privacy.html ne cite pas {manquants}"
            )

    courant = inventaire()
    if "--maj" in sys.argv:
        REFERENCE.write_text(json.dumps(courant, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"reference mise a jour : {REFERENCE.relative_to(RACINE)}")
    else:
        if not REFERENCE.exists():
            erreurs.append("pas de reference : lancer `python scripts/verifier-ecritures-disque.py --maj`")
        else:
            attendu = json.loads(REFERENCE.read_text(encoding="utf-8"))
            for fichier in sorted(set(attendu) | set(courant)):
                if attendu.get(fichier) != courant.get(fichier):
                    erreurs.append(
                        f"{fichier} : les points d'ecriture ont change (reference "
                        f"{attendu.get(fichier)}, code {courant.get(fichier)})"
                    )

    if erreurs:
        print("ECRITURES DISQUE : la politique de confidentialite est peut-etre devenue fausse.")
        for e in erreurs:
            print(f"  - {e}")
        print(
            "Relire site/privacy.html (section « Local storage » et « Remote web access ») contre le "
            "code, la corriger si besoin, puis `python scripts/verifier-ecritures-disque.py --maj`."
        )
        return 1

    print(f"ecritures disque : conforme ({len(noms_de_fichiers())} noms de fichiers cites, "
          f"{sum(len(v) for v in courant.values())} motifs inventories).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
