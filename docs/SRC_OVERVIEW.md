## Documentation du code (Rust)

Ce document décrit la **structure**, les **types** et les **fonctions** des fichiers :

- `src/main.rs`
- `src/handlers.rs`
- `src/db.rs`

Il vise à expliquer le “quoi” (API/types) et le “pourquoi” (rôle/flux) sans répéter toute l’implémentation.

### Documentation inline

Les mêmes éléments sont aussi documentés **directement dans le code** via des doc-comments Rust `///` et `//!` dans :

- `src/main.rs`
- `src/handlers.rs`
- `src/db.rs`
- `src/csrf.rs` — protection CSRF (module, types, méthodes)
- `src/models.rs` — modèles d'entrée et validation
- `src/security.rs` — limiteur d’abus par IP (score x, 429 + Retry-After)

---

## `src/main.rs` — Point d’entrée, CLI, serveur HTTP

### Rôle

- Déclare les sous-modules du binaire (`mod ...`).
- Initialise la configuration (dotenv + env vars), le logging, la base de données.
- Expose une CLI avec commandes de service et de purge.
- Construit et lance le serveur HTTP Axum, avec routes API + static.

### Modules déclarés

```text
mod csrf;
mod db;
mod handlers;
mod logging;
mod models;
mod security;
```

Ces modules sont attendus dans `src/*.rs` et sont utilisés pour :

- `csrf`: génération/vérification de token CSRF (via cookies)
- `db`: accès PostgreSQL via `sqlx`
- `handlers`: endpoints HTTP JSON
- `logging`: initialisation d’un logger (et garde de rotation)
- `models`: validation des entrées (types `SetInput`, `GetInput`, etc.)
- `security`: limiteur d’abus par IP (score décroissant, récupération dans le temps)

### Types (structures/enums)

- **`Cli`** (derive `clap::Parser`)
  - **Champs**
    - `command: Option<Command>`: sous-commande optionnelle.
  - **Rôle**: parse les arguments CLI.

- **`Command`** (derive `clap::Subcommand`)
  - **Variantes**
    - `Serve`: démarre le serveur HTTP (par défaut).
    - `PurgeOnce`: purge une fois les entrées expirées.
    - `PurgeLoop { interval_seconds: u64 }`: purge en boucle.
      - `interval_seconds` est aussi configurable via `PURGE_INTERVAL_SECONDS` (défaut 3600).

- **`AppState`** (`pub(crate)`, `Clone`)
  - **Champs**
    - `db: db::Db`: couche DB.
    - `abuse_limiter: security::AbuseLimiter`: limiteur d’abus par IP (score x ∈ [0,16], 429 si x < 1).
    - `csrf: csrf::CsrfConfig`: config CSRF (cookie + champ attendu).
    - `trust_proxy: bool`: accepte `x-forwarded-for` si `true`.
  - **Rôle**: état partagé injecté dans les handlers Axum (`State<AppState>`).

### Fonctions

- **`main() -> anyhow::Result<()>`** (async, `#[tokio::main]`)
  - **Flux**
    - Charge `.env` si présent (`dotenvy`).
    - Parse la CLI.
    - Initialise les logs via `logging::init_logging_from_env()`.
    - Connecte la DB via `db::Db::connect_from_env()` puis applique les migrations `db.migrate()`.
    - Exécute `Serve` / `PurgeOnce` / `PurgeLoop`.
  - **Erreurs**: propagées via `anyhow` avec contexte (`Context`).

- **`serve(db: db::Db, _log_guard: logging::LogGuard) -> anyhow::Result<()>`** (async)
  - **Config**
    - `APP_ADDR` (défaut `"0.0.0.0:3000"`) pour bind.
    - `TRUST_PROXY` (`"true"` active l’usage de `x-forwarded-for`).
    - CSRF via `csrf::CsrfConfig::from_env()`.
    - Limiteur d’abus: `security::AbuseLimiter::new(ttl)` avec `ABUSE_TTL_SECS` (défaut 86400) et une constante interne `c = 1.1`.
  - **Routes**
    - Static:
      - `/static/*` → `ServeDir("static")`
      - `/` → `ServeFile("static/index.html")`
    - API:
      - `GET /api/csrf` → `handlers::api_csrf`
      - `GET /api/salts` → `handlers::api_salts`
      - `POST /api/get` → `handlers::api_get`
      - `POST /api/set` → `handlers::api_set`
  - **Middlewares / couches**
    - `DefaultBodyLimit::max(250_000)` (limite taille body).
    - `CookieManagerLayer` (gestion cookies).
    - `TraceLayer::new_for_http()` (tracing HTTP).
    - `with_state(state)` (injecte `AppState`).
  - **Serve**
    - Bind un `TcpListener`, puis `axum::serve(...).with_graceful_shutdown(shutdown_signal())`.

- **`purge_loop(db: db::Db, interval: Duration) -> anyhow::Result<()>`** (async)
  - Appelle `db.purge_expired()` en boucle puis dort `interval`.
  - Log les statistiques (entrées et sels supprimés).
  - **Note**: boucle infinie (ne retourne pas en pratique).

- **`shutdown_signal()`** (async)
  - Attend `Ctrl-C` via `tokio::signal::ctrl_c()`.
  - Utilisé pour un arrêt “gracieux” du serveur.

---

## `src/handlers.rs` — Endpoints HTTP JSON

### Rôle

Expose les handlers Axum qui implémentent l’API JSON décrite dans le `README.md` :

- `GET /api/salts`
- `GET /api/csrf`
- `POST /api/set`
- `POST /api/get`

Le fichier gère aussi :

- **Validation** via `crate::models` (ex: `SetInput`, `GetInput`)
- **CSRF** pour les écritures (`/api/set`)
- **Limitation d’abus par IP** via `state.abuse_limiter.check_or_update(ip)` (score x, 429 + Retry-After si x < 1)
- **Logging** avec `tracing`
- **Résolution IP client** via header `x-forwarded-for` si `TRUST_PROXY=true`

### Types (structures)

Types **internes** (privés au module) :

- **`ApiOk`** (`Serialize`)
  - `ok: bool`
  - `error: Option<String>` (non sérialisé si `None`)
  - **Utilisation**: réponse simple “OK / erreur”.

- **`ApiGetResponse`** (`Serialize`)
  - `found: bool`
  - `value: Option<ValueEnc>` (si trouvé)
  - `error: Option<String>` (non sérialisé si `None`)

- **`ApiCsrfResponse`** (`Serialize`)
  - `csrf: String` (token CSRF)
  - `field: String` (nom du champ attendu côté client)

- **`ApiSaltsResponse`** (`Serialize`)
  - `salts: Vec<String>` (sels encodés en base64 URL-safe, sans padding)

Types **publics** (utilisés par Axum pour le body JSON) :

- **`ApiGetRequest`** (`pub`, `Deserialize`)
  - `hashes: Option<Vec<String>>`
  - **Remarque**: l’API attend une liste non vide (validation dans le handler).

- **`ApiSetRequest`** (`pub`, `Deserialize`)
  - `key_hash: Option<String>`
  - `value: Option<ValueEnc>` (payload chiffré)
  - `ephemeral: Option<bool>` (suppression à la lecture si `true`)
  - `csrf: Option<String>` (token CSRF, requis pour écrire)

- **`ValueEnc`** (`pub`, `Deserialize + Serialize + Clone`)
  - `v: u8` (version/format)
  - `iv: String` (nonce/IV encodé)
  - `ct: String` (ciphertext encodé)
  - **Rôle**: représentation “transport” d’un blob chiffré (le serveur ne déchiffre pas).

### Fonctions

- **`json<T: Serialize>(status: StatusCode, payload: T) -> Response`** (privée)
  - Petit helper qui retourne une `Response` JSON avec un status HTTP.

- **`api_salts(State(state): State<AppState>) -> Response`** (publique, async)
  - **DB**: appelle `state.db.list_valid_salts_with_rotation()`.
  - **Encodage**: convertit chaque sel (`Vec<u8>`) en base64 URL-safe sans padding.
  - **Cache**: la réponse est marquée comme **non-cachable** côté client (`Cache-Control: no-store, no-cache, must-revalidate`).
  - **Réponse**
    - `200 OK`: `{ salts: [...] }`
    - `500`: `{ ok: false, error: "Internal error: ..." }`

- **`api_csrf(State(state): State<AppState>, cookies: tower_cookies::Cookies) -> Response`** (publique, async)
  - **CSRF**: `state.csrf.issue_token(&cookies)` (pose typiquement un cookie HttpOnly et renvoie aussi un token).
  - **Réponse**
    - `200 OK`: `{ csrf: "...", field: "..." }`
    - `500`: `{ ok: false, error: "Internal error: ..." }`

- **`api_set(State(state), headers, ConnectInfo(addr), cookies, Json(req)) -> Response`** (publique, async)
  - **1) Vérif CSRF** (écriture)
    - `state.csrf.verify(&cookies, req.csrf.as_deref())`
    - En cas d’échec: `403 Forbidden` avec `{ ok:false, error:"Forbidden" }`.
  - **2) IP client + limiteur d’abus**
    - IP = `client_ip(&headers, addr, state.trust_proxy)`
    - Si `state.abuse_limiter.check_or_update(ip)` → `Err(retry_secs)` : renvoie `429` avec en-tête `Retry-After: <secs>` et `{ ok:false, error:"too many requests" }`.
  - **3) Validation**
    - `key_hash` doit être présent/non vide, sinon `400`.
    - `value` doit être présent, sinon `400`.
    - Sérialise `ValueEnc` en JSON string (stocké tel quel en DB).
    - Valide via `models::SetInput { key, value, ephemeral }.validate()`, sinon `400`.
  - **4) Logging**
    - Log `event="set"`, `ip`, longueurs (pas le contenu).
  - **5) DB**
    - `state.db.insert(&key, &value, ephemeral)` ; si erreur → `500`.
  - **Réponse**
    - `200 OK`: `{ ok:true }`
    - `429`: en-tête `Retry-After` (secondes), corps `{ ok:false, error:"too many requests" }`
    - `400/403/500`: selon cas ci-dessus.

- **`api_get(State(state), headers, ConnectInfo(addr), Json(req)) -> Response`** (publique, async)
  - **1) IP client + log**
    - IP = `client_ip(...)`
    - Log `event="get"`, `ip` uniquement.
  - **2) Limiteur d’abus**
    - Si `state.abuse_limiter.check_or_update(ip)` → `Err(retry_secs)` : renvoie `429` avec `Retry-After` et `{ found:false, error:"too many requests" }`.
  - **3) Validation**
    - `hashes` requis et non vide, sinon `400`.
    - Max 256 hashes, sinon `400`.
    - Chaque hash doit être non vide et valide via `models::GetInput { key }.validate()`.
  - **4) DB**
    - `state.db.get_value_by_hashes_maybe_delete_ephemeral(validated_hashes)`
    - Si erreur → `500`.
  - **5) Parsing**
    - Si valeur trouvée, parse le JSON en `ValueEnc`.
    - Si parsing échoue → `500` (“corrupted payload”).
  - **Réponse**
    - `200 OK`:
      - trouvé: `{ found:true, value:{ v, iv, ct }, ttl_secs, ephemeral }`
      - non trouvé: `{ found:false }`
    - `400/500`: validation / erreur interne.
    - `429`: en-tête `Retry-After`, corps `{ found:false, error:"too many requests" }`

- **`client_ip(headers: &HeaderMap, addr: SocketAddr, trust_proxy: bool) -> IpAddr`** (privée)
  - Si `trust_proxy=true`, tente `x-forwarded-for` (prend la **première** IP de la liste).
  - Sinon, fallback sur `addr.ip()` (socket peer).

---

## `src/db.rs` — Accès PostgreSQL (sqlx) + logique “TTL”

### Rôle

- Gérer la connexion `PgPool` et exécuter les migrations `sqlx`.
- Stocker des paires `(key_hash, value, ephemeral)` dans `entries`.
- Lire la valeur la plus récente (sur une liste de hashes) et supprimer si éphémère.
- Gérer des “salts” en base, avec rotation (~1h) et validité (~25h).
- Purger les données expirées (entries: 24h, salts: 25h).

### Types (structures)

- **`Db`** (`pub`, `Clone`)
  - `pool: PgPool`
  - **Rôle**: façade d’accès DB.

- **`FoundRow`** (privé, `sqlx::FromRow`)
  - `id: i64`
  - `value: String`
  - `ephemeral: bool`
  - `created_at: OffsetDateTime`
  - **Rôle**: forme de la ligne renvoyée par la requête de lecture (avec lock).

- **`SaltRow`** (privé, `sqlx::FromRow`)
  - `salt: Vec<u8>`
  - `created_at: OffsetDateTime`

- **`PurgeStats`** (`pub`, `Clone + Copy`)
  - `entries_deleted: u64`
  - `salts_deleted: u64`
  - **Rôle**: statistiques de purge exposées à `main.rs`.

### Fonctions / Méthodes

Toutes les fonctions ci-dessous retournent `anyhow::Result<...>` et ajoutent du contexte via `Context`.

- **`Db::connect_from_env() -> anyhow::Result<Self>`** (pub, async)
  - Lit:
    - `DATABASE_URL` (**requis**)
    - `DB_MAX_CONNECTIONS` (optionnel, défaut 10)
  - Configure `PgPoolOptions`:
    - `max_connections(...)`
    - `acquire_timeout(10s)`
  - Connecte au Postgres et retourne `Db { pool }`.

- **`Db::migrate(&self) -> anyhow::Result<()>`** (pub, async)
  - Lance `sqlx::migrate!("./migrations").run(&self.pool)`.
  - **Hypothèse**: migrations présentes dans `./migrations` à la racine.

- **`Db::insert(&self, key_hash: &str, value: &str, ephemeral: bool) -> anyhow::Result<()>`** (pub, async)
  - Insère dans `entries (key_hash, value, ephemeral)`.
  - Garantit qu’il n’existe **qu’une seule** entrée par `key_hash` en supprimant d’abord toute entrée existante pour cette clé, puis en insérant la nouvelle valeur (comportement “écrase l’ancienne valeur”).

- **`Db::get_value_by_hashes_maybe_delete_ephemeral(&self, key_hashes: Vec<String>) -> anyhow::Result<Option<FoundEntry>>`** (pub, async)
  - **But**: retourner la **valeur la plus récente** (sur un ensemble de `key_hash`) et la supprimer si `ephemeral=true`.
  - **Étapes**
    - Si `key_hashes` vide → `Ok(None)`.
    - Démarre une transaction.
    - Sélectionne:
      - `FROM entries`
      - `WHERE key_hash = ANY($1)`
      - `AND created_at >= now() - 24 hours` (TTL entries)
      - `ORDER BY created_at DESC`
      - `LIMIT 1`
      - `FOR UPDATE` (verrou de ligne)
    - Si rien trouvé: commit “best effort” puis `Ok(None)`.
    - Si trouvé et `ephemeral=true`: `DELETE FROM entries WHERE id = $1` dans la même transaction.
    - Commit puis `Ok(Some(FoundEntry { value, ephemeral, created_at }))`.

- **`Db::list_valid_salts_with_rotation(&self) -> anyhow::Result<Vec<Vec<u8>>>`** (pub, async)
  - **Rotation**: s’assure qu’un sel récent existe.
    - Lit `created_at` du sel le plus récent.
    - Si absent ou plus vieux que ~1h → génère 16 octets aléatoires (`rand::RngCore`) et `INSERT INTO salts (salt)`.
  - **Liste**: retourne tous les sels `created_at >= now() - 25 hours` triés du plus récent au plus ancien.
  - **Retour**: `Vec<Vec<u8>>` (les bytes bruts; l’API encode en base64 dans `handlers.rs`).

- **`Db::purge_expired(&self) -> anyhow::Result<PurgeStats>`** (pub, async)
  - `DELETE FROM entries WHERE created_at < now() - 24 hours`
  - `DELETE FROM salts WHERE created_at < now() - 25 hours`
  - Retourne `PurgeStats` avec `rows_affected()` pour chaque DELETE.

---

## `src/security.rs` — Limitation d’abus par IP

### Rôle

Chaque IP est associée à un score flottant **x** ∈ [0, 16]. À chaque requête :
- **Mise à jour** : x ← (x/2) × c^dt (dt = temps en secondes depuis la dernière requête), puis x est borné dans [0, 16].
- Si **x < 1** : la requête est refusée avec **429** et l’en-tête **Retry-After** (secondes) = (ln(2) − ln(x)) / ln(c). L’état n’est pas mis à jour.
- Sinon : la requête est acceptée et le nouvel état (x, instant) est enregistré.

Variables d’environnement :
- **`ABUSE_TTL_SECS`** (défaut 86400) : TTL du cache ; après expiration l’IP est oubliée et repart à 16.
- **Constante interne** : `c = 1.1` (non configurable par variable d’environnement).

### Types

- **`AbuseLimiter`** (`Clone`) : cache `IpAddr → (x, last_seen)` (moka), + constante `c`.
- **`check_or_update(&self, ip: IpAddr) -> Result<(), u64>`** : `Ok(())` si autorisé (et mise à jour), `Err(retry_after_secs)` si 429 doit être renvoyé.

