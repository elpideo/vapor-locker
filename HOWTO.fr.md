# Vapor

Service web (Rust + Axum) de stockage clé‑valeur avec récupération, durée de vie de 24h, option **éphémère** (suppression à la lecture), CSRF, anti‑attaque via cache IP (3s) et logs JSON rotatifs.

## Documentation du code

- Voir `docs/SRC_OVERVIEW.md` (structure + types + fonctions de `src/main.rs`, `src/handlers.rs`, `src/db.rs`)

## Prérequis

- Rust stable (toolchain 2021)
- PostgreSQL
- Docker + Docker Compose (si déploiement via conteneurs)

## Configuration

Variables d’environnement (exemples) :
Le binaire charge automatiquement un fichier `.env` s’il est présent.

- `DATABASE_URL=postgres://postgres:postgres@localhost:5433/vapor`
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

## Front (statique)

- **Page unique (UI)**: `GET /` sert `static/index.html`
- **Bouton GitHub** : icône en haut à droite, lien vers le dépôt source
- **Fichiers statiques**: `GET /static/*` (JS, etc.)
- **Paramètre d'URL `key`** : si l'utilisateur arrive avec `?key=...` dans l'URL (ex. `https://vapor-locker.com?key=ma-cle`), la valeur est pré-remplie dans le champ Key de la section RETRIEVE.
- **Section STORE** : ordre des champs — d'abord le champ **Key** (avec icône copier dans le champ et bouton pour générer une clé aléatoire), puis le champ **Value** (contenu / secret).
- **Clé aléatoire**: dans la section STORE, un bouton icône à droite du champ Key génère une clé aléatoire (64 caractères alphanumériques cryptographiquement sécurisés). Une icône copier discrète à l'intérieur du champ permet de copier la clé dans le presse-papier.
- **Option éphémère**: “EVAPORATING CONTENT” supprime la valeur après la première lecture (case à cocher et libellé mis en avant dans la couleur d’accent verte).
- **Après STORE**: après enregistrement réussi, le champ valeur est estompé (fadeout) puis vidé. Sous la ligne **EVAPORATING CONTENT** s'affichent une coche de validation, le libellé **“evaporating in”** puis un **compte à rebours 24h** (format `HH:MM:SS`, police monospace), puis le lien vers `https://vapor-locker.com?key=...` et une icône pour copier ce lien (pas de message « OK »).
- **Masquage du résultat**: quand une valeur est trouvée, l’UI affiche `*******` par défaut. Une icône “œil” permet d’afficher/masquer la valeur, et l’icône “copier” copie toujours la vraie valeur.
- **TTL après RETRIEVE**: quand une valeur est trouvée, le libellé **“evaporating in”** puis un compte à rebours (temps restant en base, `HH:MM:SS`) s’affiche **sous la valeur**. Si la valeur est éphémère, l’UI joue une animation rapide qui fait passer de `23:59:59` à `00:00:00` en ~2 secondes.
- **Sauts de ligne**: l’affichage et la copie conservent les retours à la ligne du contenu stocké.
- **Sections informatives** (sous le formulaire) : HOW IT WORKS, SECURITY MODEL, WHAT THE SERVER SEES, THREAT MODEL, DESIGN PRINCIPLES, SOURCE (lien GitHub).

## API (JSON)

- `GET /api/csrf` → `{ "csrf": "...", "field": "csrf" }` (pose aussi le cookie CSRF HttpOnly)
- `GET /api/salts` → `{ "salts": ["..."] }`
  - Retourne les sels valides (créés dans les **25 dernières heures**), triés du plus récent au plus ancien.
  - Crée automatiquement un nouveau sel si le plus récent a plus de ~1h.
- `POST /api/set` (JSON) → `{ "ok": true }`
  - Le navigateur envoie uniquement:
    - `key_hash`: un hash dérivé de (clé + sel)
    - `value`: une valeur **chiffrée** `{ v, iv, ct }` (AES‑GCM)
- `POST /api/get` (JSON) → `{ "found": true, "value": { "v": 1, "iv": "...", "ct": "..." }, "ttl_secs": 123, "ephemeral": false }` ou `{ "found": false }`
  - Le navigateur envoie une liste de `hashes` (un par sel valide).
  - Le serveur ne voit jamais la clé en clair et ne renvoie que du chiffré.
  - En cas de limitation par IP: `429` avec `{ "error": "to many request" }` (même message côté STORE et RETRIEVE).

Exemple minimal (set):
(la dérivation PBKDF2‑SHA256 (200k itérations) et le chiffrement AES‑GCM sont faits dans `static/app.js` via WebCrypto)

```js
const { csrf } = await (await fetch('/api/csrf')).json();
const { salts } = await (await fetch('/api/salts')).json();
// key_hash + value doivent être calculés côté navigateur (voir static/app.js)
const res = await fetch('/api/set', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ key_hash: '...', value: { v: 1, iv: '...', ct: '...' }, ephemeral: false, csrf }),
});
console.log(await res.json());
```

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

> **Déploiement en production** : voir `Deploiement.md` pour pousser vers le registry et déployer sur un serveur.

### Démarrage

Depuis la racine du projet :

```bash
docker compose up --build
```

Puis ouvrir:

- `http://localhost:3000/` (entrée)
- `http://localhost:3000/static/app.js` (asset statique)

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

Les sels expirés sont aussi purgés : `created_at < now() - 25h`.

### Logs

Les logs sont en JSON (une ligne par événement) et tournent par taille. En plus du fichier, les logs sont aussi envoyés sur stdout pour `docker compose logs` :

- `LOG_MAX_BYTES` (défaut 100MB)
- `LOG_MAX_FILES` (défaut 5)

Dans Compose, ils sont écrits dans le volume `app_logs` (chemin container `/app/logs`).

