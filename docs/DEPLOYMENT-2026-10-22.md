# Vision-Pro — déploiement Internet du 22 octobre 2026

## Adresse et cible

- Site public : https://vision-pro.34-73-186-88.sslip.io
- Relais à saisir dans Vision-Pro Windows : `vision-pro.34-73-186-88.sslip.io` (WSS/443).
- VM : `zaalis-free-e2-micro`, projet `zaalis-504018`, zone `us-east1-b`, compte opérateur `zaalisblockchain@gmail.com`.
- Identité VM vérifiée : `8497323233124612375`.
- IP externe : `34.73.186.88`, réservée sous `vision-pro-public-ip` en région `us-east1` et attachée à cette VM.
- Avant installation : aucun service applicatif Zaalis, `/opt` vide, seuls SSH et les services système écoutaient. L'autre VM `vm-free-tier`, sur l'autre compte, n'a pas été modifiée.

Le nom DNS utilise sslip.io, qui résout l'IP encodée dans le nom. Aucun compte DNS, achat de domaine, identifiant DuckDNS ou tunnel temporaire n'a été utilisé. La réservation de l'IP empêche un changement de nom lors d'un arrêt/redémarrage normal de la VM. Le service reste dépendant de Google Cloud, de sa facturation et de la disponibilité du DNS tiers. Le nom gratuit ne signifie pas que les ressources Google Cloud sont gratuites.

## Installation effectuée

- Version distante : `/opt/vision-pro/releases/20261022T154846Z`, lien actif `/opt/vision-pro/current`.
- Sauvegarde de la configuration Nginx précédente : `/opt/vision-pro/backups/20261022T154846Z/nginx`.
- Archive transférée vérifiée par SHA-256 : `A77C35E020831E6F9D5D02CB725022F06A7DE3A273958B7BF6DF97169DFCC9B2`.
- Node.js 22.23.4 téléchargé depuis nodejs.org, archive comparée aux sommes SHA-256 officielles. Dépendances du service de comptes installées depuis le verrou propre à l'archive.
- Binaire du relais compilé sous Linux à partir des sources locales et du verrou Rust corrigé.
- Services `vision-pro-account`, `vision-pro-relay` et `nginx` actifs, démarrage automatique activé. `Restart=on-failure` pour les deux services Vision-Pro ; compteur de redémarrages nul aux contrôles.
- Service de comptes sur `127.0.0.1:3000`, relais sur `127.0.0.1:18080`. Les ports TURN historiques sont uniquement sur la boucle locale et ne sont pas utilisés par la tablette graphique actuelle.
- Nginx expose HTTPS/443 et redirige HTTP/80 vers HTTPS ; `/v1/` transmet les WebSockets au relais sur la même origine, compatible avec la CSP restrictive du site.
- Certificat Let's Encrypt valide jusqu'au 20 janvier 2027, clé privée conservée sur la VM. Timer Certbot activé ; hook de rechargement Nginx installé pour les renouvellements.
- Simulation de renouvellement `certbot renew --dry-run` réussie pour ce certificat, avec validation du challenge par Let's Encrypt staging.
- Services applicatifs sous l'utilisateur système dédié `vision-pro`, restrictions systemd, plafonds mémoire, stockage des comptes hors des sources dans `/var/lib/vision-pro`, répertoire mode 700 et configuration mode 600.
- Inscriptions publiques activées avec les limites et validations du service de comptes. Le compte web est facultatif pour le contrôle d'un PC ; l'autorisation repose sur l'appairage et les clés des appareils.
- Relais ouvert aux inscriptions cryptographiquement signées des PC Vision-Pro, sans jeton partagé imposé. Ce choix permet l'usage public demandé ; connaître l'URL ne permet pas de contrôler un PC sans son appairage.

## Modifications pour l'accès public

Le formulaire « Autre ordinateur » sur l'origine publique accepte l'identifiant du PC, son empreinte complète et le code d'appairage affiché sur le PC. Il ouvre le WebSocket du relais sur cette même origine, vérifie la clé attendue, puis mémorise la clé et le relais pour les connexions suivantes. Les données IndexedDB d'une ancienne origine locale ne sont pas automatiquement transférées ; le premier appairage sur le site public reste nécessaire.

La version Windows affiche maintenant l'empreinte complète dans « À propos », en complément de l'empreinte courte existante.

## Preuves de fonctionnement

- Depuis Windows, HTTPS vérifié sans contournement de certificat : page d'accueil HTTP 200, `/healthz` répond `ok`, configuration des comptes accessible.
- Requête d'inscription avec origine étrangère refusée par le service.
- TypeScript, 22 tests JavaScript/TypeScript et construction des deux interfaces réussis ; compilation Windows de production et installateur NSIS réussis ; `cargo fmt --all --check` et `git diff --check` passent.
- Test client TypeScript contre un véritable agent Windows isolé à travers le relais public : PC inexistant refusé, appairage sans code refusé, code correct accepté, réauthentification acceptée, trame chiffrée altérée rejetée, mauvaise empreinte rejetée, appareil non appairé refusé. État de test supprimé ensuite ; aucune identité de test ajoutée au profil habituel du PC.
- Test du véritable site public avec Edge/Chromium en format iPad 834 px : contexte sécurisé, formulaire accessible, aucune erreur JavaScript ni débordement horizontal. Ce navigateur a ensuite appairé un véritable agent Windows isolé via le formulaire déployé et mémorisé son empreinte et son relais dans IndexedDB.
- L'application Windows normale a été lancée avec la configuration du relais public. Son enregistrement a été observé dans le journal distant du relais. Les identités et appareils existants sont préservés.
- Ces tests ne remplacent pas un essai sur l'iPad physique et Safari/iPadOS, ni un audit de sécurité indépendant. L'application actuelle transmet les commandes de tablette graphique/trackpad ; elle ne diffuse pas l'écran du PC.

## Version Windows livrée

Fichiers dans `installer/internet-20261022/` :

| Fichier | SHA-256 |
| --- | --- |
| `vision-pro.exe` | `B679C4B1C26C1812065C76F42E83BD94ADD66BC7C01ACC92FB381F4A8A4397F3` |
| `Vision-Pro_0.1.0_x64-setup.exe` | `A8E5C48BB538A86D3EBF1B9EBD4F4440C17FD7A64BAABFE24BD2BC3BCD42F942` |

Cette version est non signée (`NotSigned`). La version portable a été lancée depuis ce dossier. L'installateur est fourni mais n'a pas été exécuté. Les anciens livrables à la racine d'`installer/` restent conservés.

Réglages du PC : mode `remote-access`, relais `wss://vision-pro.34-73-186-88.sslip.io`. Sauvegarde précédente : `%APPDATA%\Vision-Pro\settings.json.pre-internet-20261022`. Le lancement automatique de Windows n'a pas été activé ; ouvrir Vision-Pro après ouverture de session si nécessaire.

## Utilisation sur iPad

1. Garder le PC allumé, connecté à Internet, avec la nouvelle version Vision-Pro ouverte. Dans Réseau, l'accès à distance doit indiquer que le relais est joignable.
2. Ouvrir https://vision-pro.34-73-186-88.sslip.io dans Safari. Nommer l'iPad et choisir « Commencer ». L'ajout à l'écran d'accueil est facultatif.
3. Choisir « Autre ordinateur ». Sur le PC, ouvrir « À propos » et recopier l'Agent ID ainsi que « Empreinte pour connexion Internet » dans les deux champs correspondants.
4. Sur le PC, choisir « Connecter un appareil ». Entrer sur l'iPad le code à six chiffres. Le code autorise cet appareil ; le garder privé.
5. Aux utilisations suivantes, ouvrir le même site, choisir le PC mémorisé et « Connecter ». La connexion fonctionne aussi depuis un autre Wi-Fi ou un partage de connexion : aucun réseau local commun n'est nécessaire. Les deux appareils doivent néanmoins disposer d'Internet.

Un compte créé sur le site ne donne pas accès aux PC des autres utilisateurs. Pour retirer l'accès d'un iPad, le supprimer dans les appareils autorisés sur le PC. Le raccourci d'urgence reste `Ctrl+Alt+Shift+Backspace`.

## Exploitation et retour arrière

Inspecter les services avec `systemctl status vision-pro-account vision-pro-relay nginx` et leurs journaux avec `journalctl -u vision-pro-account -u vision-pro-relay`. Les journaux comportent des métadonnées d'appareils/IP et ne doivent pas être publiés.

Le script `scripts/deploy-native.sh` décrit l'installation. Avant une future mise à jour, conserver le lien de version active et une sauvegarde privée du fichier des comptes. Pour rétablir une version ultérieurement remplacée : repointer `/opt/vision-pro/current` vers la version connue, puis redémarrer les deux services Vision-Pro. Les données des comptes restent à l'extérieur des versions.

Pour annuler ce premier déploiement, arrêter/désactiver les services Vision-Pro, retirer uniquement le lien Nginx `vision-pro`, restaurer au besoin la configuration Nginx sauvegardée et valider `nginx -t` avant rechargement. Conserver `/var/lib/vision-pro` et les certificats pour récupération. Aucun contenu Zaalis n'a été supprimé dans cette intervention.
