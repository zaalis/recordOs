# Connexion simplifiée 0.1.1 — 22 octobre 2026

Le parcours public utilise exclusivement le relais Internet : adresse publique sur Windows, code temporaire à quatre chiffres sur l'iPad, puis confirmation explicite sur le PC. Aucun compte n'est nécessaire. L'identifiant et l'empreinte du PC sont récupérés automatiquement. Les interfaces ne proposent plus de découverte Wi-Fi, d'adresse IP locale ou d'installation de profil de confiance. Les anciens comptes et les données du profil PC restent conservés.

## Sécurité du jumelage

Le PC publie une invitation sur sa connexion WSS déjà authentifiée par signature de son identité. Le relais associe le code à l'identifiant et l'empreinte issus de cette identité vérifiée. Le code expire après 120 secondes ; les collisions entre PC entraînent une régénération. Les invitations sont bornées et supprimées à la déconnexion. La recherche se fait par POST JSON sur `/v1/pair` pour éviter les codes dans les URL des journaux d'accès ; réponse `no-store`. Le relais connaît temporairement le code et les métadonnées de l'invitation, jamais les clés privées des appareils.

Recherche limitée à cinq requêtes par IP avec recharge lente, et cinq résolutions par invitation ; republier le même code ne remet pas ce compteur à zéro. Côté agent, cinq preuves invalides bloquent le code jusqu'à un renouvellement explicite. Une preuve correcte consomme le code puis attend la confirmation Windows. Le code de comparaison issu de la session doit correspondre sur les deux écrans. Aucune autorisation n'est enregistrée avant acceptation ; un échec d'écriture du registre rétablit son état antérieur et refuse la session. Les connexions ultérieures utilisent les clés mémorisées et vérifient l'empreinte du PC.

Le HTTPS public conserve son certificat reconnu par le navigateur. Les anciennes fonctions locales restent dans le code pour compatibilité, mais le parcours public ne les utilise plus ; le démarrage migre les réglages vers l'accès distant, sans découverte locale ni USB direct.

## Validation

- TypeScript, compilation des interfaces et installateur NSIS Windows 0.1.1. Version portable finale lancée dans un profil isolé : agent Windows et identité servis correctement, puis processus de test arrêté.
- 22 tests JavaScript/TypeScript ; 47 tests unitaires agent, 14 tests unitaires relais, cinq tests d'intégration relais, vecteurs cryptographiques interlangages et reconnexion agent/relais.
- Test client sur le relais public : résolution du code, PC hors ligne, jumelage sans preuve refusé, attente de confirmation, jumelage accepté, réauthentification, mauvaise identité refusée, appareil inconnu refusé et trame chiffrée altérée rejetée.
- Navigateur au format iPad contre un agent Windows isolé : aucune mémorisation avant confirmation, refus PC, nouvelle tentative approuvée, clé et relais persistés, aucune erreur JavaScript.
- Affichage testé à 375×812, 834×1194 et 1194×834, thèmes clair/sombre et mouvement réduit ; aucun débordement horizontal.
- Safari/iPad physique : validation utilisateur restante. Installateur non exécuté, binaires non signés.

## Serveur

Cible inchangée et vérifiée par l'identifiant VM 8497323233124612375. Nouvelle version `/opt/vision-pro/releases/20261022T162744Z-pairing011`. Les services compte, relais et Nginx sont actifs. Les données utilisateurs et les paramètres TLS restent en place. Version initiale conservée : `/opt/vision-pro/releases/20261022T154846Z`. Sauvegarde intermédiaire : `/opt/vision-pro/releases/20261022T162420Z-pairing011`.

Archive livrée : SHA-256 `9F842F474AF65F0CDF4CED7098B0382D6C5D37DDD23A07D3EE5C54FB1CF324D6`.
