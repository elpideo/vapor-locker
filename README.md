# Vapor

Service web (Rust + Axum) de stockage clé‑valeur avec récupération, durée de vie de 24h, option **éphémère** (suppression à la lecture), CSRF, anti‑attaque via cache IP (3s) et logs JSON rotatifs.

## Prérequis

- Rust stable (toolchain 2021)
- PostgreSQL
- Docker + Docker Compose (si déploiement via conteneurs)

## Configuration

Variables d’environnement (exemples) :
Le binaire charge automatiquement un fichier `.env` s’il est présent.

- `DATABASE_URL=postgres://postgres:postgres@localhost:5432/vapor`
- `APP_ADDR=0.0.0.0:3000`
- `TRUST_PROXY=false`
- `COOKIE_SECURE=false`
- `LOG_DIR=logs`
- `LOG_FILE=vapor.log`
- `LOG_MAX_BYTES=104857600` (100MB)
- `LOG_MAX_FILES=5`

## Lancer en local (sans Docker)

1. Créer la base PostgreSQL `vapor` (et l’utilisateur si besoin).
2. Copier/adapter le fichier `.env` à la racine du projet.
3. Lancer :

```bash
cargo run
```

Au démarrage, les migrations SQL dans `migrations/` sont appliquées automatiquement.

## Pages

- **Page unique (UI)**: `GET /` et `GET /get` affichent la même interface (3 sections)
- **Section 1 (toujours affichée)**: formulaire **GET** (`POST /get`) + (optionnel) résultat / message
  - Résultat d’un **GET** (valeur + bouton **copier**)
  - Message **OK** après un **SET** réussi
  - Message **Non trouvé** si la clé est inconnue ou expirée
- **Section 3 (toujours affichée)**: formulaire **SET** (`POST /set`)
- **Option éphémère**: “EVAPORATING CONTENT” affiche un tooltip: “Content will be evaporated after the first reading”.
- **Masquage du résultat**: quand une valeur est trouvée, la réponse affiche `*******` par défaut. Une icône “œil” permet d’afficher/masquer la valeur, et l’icône “copier” copie toujours la vraie valeur.
- **Sauts de ligne**: l’affichage et la copie conservent les retours à la ligne du contenu stocké.

## Purge

Suppression des entrées expirées (24h) :

```bash
cargo run -- purge-once
```

ou en boucle :

```bash
cargo run -- purge-loop --interval-seconds 3600
```

## Docker / Docker Compose

### Démarrage

Depuis la racine du projet :

```bash
docker compose up --build
```

Puis ouvrir:

- `http://localhost:3000/` (entrée)
- `http://localhost:3000/get` (récupération)

### Base de données

Le `docker-compose.yml` crée automatiquement :

- DB: `vapor`
- user: `vapor`
- password: `vapor`

Par défaut, PostgreSQL n'est pas exposé à l'hôte. Pour y accéder :

```bash
docker compose exec db psql -U vapor -d vapor
```

### Purge “cron”

Le service `purger` lance :

- `vapor purge-loop` toutes les `PURGE_INTERVAL_SECONDS` (défaut 3600s)

et supprime les entrées `created_at < now() - 24h`.

### Logs

Les logs sont en JSON (une ligne par événement) et tournent par taille :

- `LOG_MAX_BYTES` (défaut 100MB)
- `LOG_MAX_FILES` (défaut 5)

Dans Compose, ils sont écrits dans le volume `app_logs` (chemin container `/app/logs`).

