# Record OS 0.1.5 — rapport de livraison Windows

19 novembre 2026

## Application Windows

Le tableau de bord reprend la maquette sombre monochrome. Il affiche le nombre d’appareils, la latence de la session, les appareils associés et l’activité récente à partir du service Windows. Le grand bandeau de session et l’indicateur de sécurité du tableau de bord ont été retirés. Le logo d’origine figure dans la barre latérale et sur l’écran À propos. Les commandes de déconnexion sont accessibles dans **Appareils**.

## Mises à jour GitHub

Record OS vérifie GitHub Releases au démarrage puis toutes les 30 minutes. Une nouvelle version est téléchargée en arrière-plan et le paquet est vérifié avec la clé publique intégrée à l’application. Au-dessus du nom du PC, l’indicateur ouvre une fenêtre avec la progression. Dès que le paquet est prêt, **Mise à jour** déclenche l’installation et affiche le message de redémarrage. Si le service était actif, il est arrêté proprement avant de lancer le programme d’installation. L’installateur NSIS redémarre ensuite l’application.

Le canal public lit l’asset `latest.json` de la dernière version stable publiée du dépôt [zaalis/recordOs](https://github.com/zaalis/recordOs/releases). Le workflow **Préparer une version Windows signée** construit le paquet, utilise les secrets Actions `RECORD_OS_UPDATER_KEY` et `RECORD_OS_UPDATER_PASSWORD`, et joint le manifeste et les deux installateurs à un brouillon GitHub Releases. Le brouillon peut être vérifié avant de publier la version. La signature du paquet est celle de l’updater Tauri.

La 0.1.5 introduit l’updater. Les installations précédentes doivent passer une fois par l’installateur Windows pour entrer dans ce canal. Le push des sources ne publie pas une release GitHub.

## Gestes et Apple Pencil

Dans la session iPad, ouvrir **Commandes → Apple Pencil et gestes**. En mode tactile direct, activer **Gestes créatifs au doigt** pour un iPad sans Pencil. Avec Photoshop au premier plan et son profil actif, le toucher simple ouvre la palette de raccourcis et le double tap alterne entre le pinceau et la gomme. Chaque geste peut être modifié ou désactivé. Le mode tactile direct continue d’envoyer les glissés de souris au PC. Le réglage est enregistré sur l’iPad, avec les préférences de la session.

Pour les stylets compatibles avec les événements de pression et Windows Ink, la sensibilité va de 50 à 200 %, avec 115 % par défaut et trois courbes de pression. Le premier point conserve sa pression mesurée. Les boutons physiques du Pencil accessibles via une intégration iPadOS native ne sont pas capturés par la PWA.

## Vérifications

- 32 tests JavaScript et TypeScript réussis, dont le clic, la distinction toucher simple/double tap, l’annulation du geste et la pression.
- Typecheck des espaces de travail et builds Windows et iPad réussis.
- Installateur x64 personnalisé généré et paquet NSIS signé. L’autovérification du programme d’installation contrôle le SHA-256 du paquet embarqué et sa version. La clé publique embarquée valide la signature du paquet original et rejette un fichier modifié.
- Le binaire natif a démarré avec un profil de test isolé. Les huit écrans, le logo, le mode sombre et l’API Tauri ont été vérifiés.
- La mise à jour depuis une release publique, le remplacement de l’installation existante, et les gestes sur un iPad physique dans Photoshop restent à valider.

Le programme d’installation assemblé localement est `installer/Record-OS-0.1.5/Record-OS_0.1.5_x64-setup.exe`. Le fichier `SHA256SUMS.txt` dans ce dossier contient les empreintes des binaires locaux.
