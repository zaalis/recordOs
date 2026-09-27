# Vision-Pro pour macOS

Vision-Pro transforme un iPad en surface de contrôle sécurisée et à faible latence pour un **Mac**. Le chemin local est direct : l'agent macOS gère la signalisation authentifiée, la capture d'écran (ScreenCaptureKit), l'encodage matériel (VideoToolbox), le flux WebRTC et l'injection des commandes (souris, clavier, Apple Pencil avec pression et inclinaison) sans jamais faire transiter la vidéo par un serveur.

Ce dossier est la version **100 % macOS** du projet : aucun code, dépendance ni cible Windows.

## Contenu du projet

| Chemin | Rôle |
| --- | --- |
| `apps/ipad` | PWA iPadOS/Safari : association, réception WebRTC, toucher, trackpad, Apple Pencil, profils créatifs, qualité, statistiques |
| `apps/desktop` | Application Mac native (Tauri + WKWebView) : service, appareils, écrans, diffusion, commandes, sécurité, réseau, autorisations macOS |
| `agent/crates/vp-agent` | Agent Rust : mDNS/Bonjour, signalisation WebSocket authentifiée, association, sessions, WebRTC |
| `agent/crates/vp-agent/native` | Pont Objective-C vers ScreenCaptureKit, VideoToolbox, Quartz Event Services, Keychain, IOKit |
| `agent/crates/vp-agent/src/platform` | Couche macOS en Rust (écrans, capture, encodeur, injection, clavier, Keychain, autorisations) |
| `packages/protocol` | Protocole TypeScript partagé (binaire + signalisation), cryptographie, validation, profils d'apps Mac |
| `packages/design` | Composants React et icônes partagés |
| `server` | Service de comptes optionnel ; ne transporte jamais la vidéo locale |
| `docs` | Architecture, sécurité |

## Prérequis

- **macOS 13 Ventura ou plus récent** (Apple silicon M1–M4 ou Mac Intel)
- **Outils de ligne de commande Xcode** : `xcode-select --install`
- **Rust stable** (via [rustup](https://rustup.rs)) — les cibles `aarch64-apple-darwin` et `x86_64-apple-darwin` sont déclarées dans `rust-toolchain.toml`
- **Node.js 22.12 ou plus récent**
- iPad avec **Safari** (iPadOS 17 ou plus récent recommandé)

## Compiler sur le Mac

Copiez le dossier `vision-pro-macos` sur le Mac, ouvrez le Terminal dans ce dossier puis :

```bash
bash scripts/setup-macos.sh
```

```bash
npm run build:mac
```

Résultat :

- `apps/desktop/src-tauri/target/release/bundle/macos/Vision-Pro.app`
- `apps/desktop/src-tauri/target/release/bundle/dmg/Vision-Pro_0.1.0_*.dmg`

Pour un binaire **universel** (Apple silicon + Intel) :

```bash
npm run build:mac:universal
```

Compilation complète avec tous les tests (TypeScript, Rust) avant l'empaquetage :

```bash
bash scripts/build-macos.sh
```

> L'agent embarque l'app iPad compilée (`apps/ipad/dist`). Les commandes `build:mac` le font automatiquement ; si vous lancez `cargo` directement, exécutez d'abord `npm run build:web`.

## Premier lancement : autorisations macOS

macOS protège l'écran et les entrées. Au premier lancement, ouvrez **Vision-Pro → Sécurité → Autorisations macOS** et autorisez :

1. **Enregistrement de l'écran** (Réglages Système → Confidentialité et sécurité → Enregistrement de l'écran et de l'audio système) — nécessaire pour diffuser l'écran.
2. **Accessibilité** (Réglages Système → Confidentialité et sécurité → Accessibilité) — nécessaire pour que l'iPad contrôle le pointeur, le clavier et l'Apple Pencil.
3. **Réseau local** — macOS le demande automatiquement lors de la première connexion d'un iPad.

Quittez puis rouvrez Vision-Pro si macOS le demande après avoir coché une autorisation.

L'application n'est pas signée avec un certificat Developer ID : elle est signée « ad hoc » à la compilation. Si vous la copiez sur un autre Mac, ouvrez-la avec **clic droit → Ouvrir** la première fois. Après chaque recompilation, macOS peut demander de réactiver les autorisations (l'empreinte de l'app change). Pour distribuer l'app, signez-la et notarisez-la (variables `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` reconnues par Tauri).

## Développement

Application de bureau avec rechargement à chaud :

```bash
npm run dev:mac
```

Agent sans interface (console) — les demandes d'association s'affichent dans le terminal, tapez `y` ou `n` :

```bash
npm run agent -- --connect
```

Le terminal qui lance l'agent doit lui aussi avoir les autorisations Enregistrement de l'écran et Accessibilité.

Interface iPad seule :

```bash
npm run dev:ipad
```

## Vérifications

```bash
npm run typecheck
```

```bash
npm test
```

```bash
npm run test:agent
```

Le test du Keychain touche au trousseau de session ; il est ignoré par défaut :

```bash
cargo test --manifest-path agent/Cargo.toml -- --ignored
```

## Ce qui est spécifique à macOS

| Fonction | Implémentation macOS |
| --- | --- |
| Capture d'écran | ScreenCaptureKit : image en mémoire GPU (IOSurface), mise à l'échelle et conversion NV12 BT.709 par le WindowServer, uniquement les images modifiées |
| Encodage | VideoToolbox matériel (moteur multimédia Apple silicon ou Quick Sync Intel), H.264 High / HEVC Main, contrôle de débit faible latence, IDR à la demande ; repli logiciel OpenH264 |
| Pointeur | Position et forme (`NSCursor`) envoyées à part : l'iPad dessine le curseur sans la latence vidéo |
| Souris / trackpad | Quartz Event Services : vrais glissers, double et triple clics, deltas relatifs, défilement au pixel |
| Pincement | Geste natif « magnify » de macOS (comme un trackpad Mac) ; molette pour Blender/Maya/ZBrush/Houdini |
| Apple Pencil | Stylet de tablette macOS : pression, inclinaison, rotation, gomme, proximité |
| Clavier | Touches Mac (⌘ ⌥ ⌃ ⇧), touches fn, pavé numérique, touches multimédia ; gestes à 3 doigts → Mission Control, Spaces, sélecteur d'apps |
| Profils créatifs | Détectés par identifiant de bundle : Blender, Maya, ZBrush, Houdini, Photoshop — raccourcis ⌘ |
| Secrets | Identité et autorité locale chiffrées (AES-256-GCM) avec une clé stockée dans le trousseau de session |
| Données | `~/Library/Application Support/Vision-Pro` (droits 0700/0600) |
| Veille | Assertion IOKit pendant une session |
| Démarrage | LaunchAgent « Ouvrir à l'ouverture de session » ; fermer la fenêtre garde le service actif, ⌘Q quitte |
| Arrêt d'urgence | Raccourci global ⌘⌥⇧⌫ |

## Service de comptes optionnel

Le service de comptes écoute sur `127.0.0.1` et doit être placé derrière un proxy TLS. Il n'est pas nécessaire pour le contrôle direct en réseau local.

```bash
VP_ACCOUNT_PORT=3000 VP_ACCOUNT_DATA="$HOME/Vision-Pro/users.json" VP_WEB_ROOT="$PWD/apps/ipad/dist" VP_ALLOW_REGISTRATION=0 npm run start --workspace @vision-pro/server
```

## Limites actuelles

La découverte locale, l'association sécurisée, la diffusion WebRTC directe et le contrôle à distance sont implémentés. L'accès à distance via relais est prévu dans l'architecture mais volontairement inactif sans relais authentifié. macOS n'a pas d'encodeur AV1 matériel : l'agent négocie H.264 ou HEVC.

Voir [architecture](docs/ARCHITECTURE.md) et [modèle de sécurité](docs/SECURITY.md).
